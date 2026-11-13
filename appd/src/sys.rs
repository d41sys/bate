//! Thin wrappers over the libc calls the service needs.

use std::ffi::{CStr, CString, OsStr};
use std::fs::{self, File, OpenOptions};
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::net::UnixStream;
use std::path::Path;

/// Creates `dir` (and parents) and forces mode 0700 on it.
/// Held while a program is started, and while descriptors are made that only become
/// close-on-exec a moment later (macOS has no `pipe2`): a program started in between would
/// keep them.
pub static SPAWN: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub fn create_private_dir(dir: &Path) -> io::Result<()> {
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)?;
    fs::set_permissions(dir, fs::Permissions::from_mode(0o700))
}

/// Takes an exclusive, non-blocking `flock`. `Ok(None)`: another process holds
/// it. The lock lives as long as the returned file.
pub fn try_lock(path: &Path) -> io::Result<Option<File>> {
    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .mode(0o600)
        .open(path)?;
    // SAFETY: the descriptor belongs to `file`, which outlives the call.
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
        return Ok(Some(file));
    }
    let err = io::Error::last_os_error();
    if err.kind() == io::ErrorKind::WouldBlock {
        Ok(None)
    } else {
        Err(err)
    }
}

/// True when the peer on `stream` runs as the service's own user.
pub fn peer_is_self(stream: &UnixStream) -> bool {
    let (mut uid, mut gid) = (0, 0);
    // SAFETY: valid descriptor and valid out-pointers.
    let rc = unsafe { libc::getpeereid(stream.as_raw_fd(), &mut uid, &mut gid) };
    // SAFETY: `geteuid` has no preconditions.
    rc == 0 && uid == unsafe { libc::geteuid() }
}

/// Random lowercase hex string of `bytes * 2` characters.
pub fn random_hex(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    // SAFETY: the pointer and length describe `buf`.
    unsafe { libc::arc4random_buf(buf.as_mut_ptr().cast(), buf.len()) };
    buf.iter().map(|b| format!("{b:02x}")).collect()
}

/// Unix milliseconds now.
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

/// One line to the service log (`crate::log`).
macro_rules! log {
    ($($arg:tt)*) => { $crate::log::write(&format!("{} {}", $crate::sys::now_ms(), format_args!($($arg)*))) };
}
pub(crate) use log;

/// Most names `Dir::names` gives: the bot decides how many there are, and the service's one
/// thread reads them.
const MAX_NAMES: usize = 10_000;

/// A folder held open. A bot can swap anything in its own folder for a link at any moment, so
/// the service reaches in only through this: what is done here stays in this very folder, and
/// a name in it is never followed if it is a link.
pub struct Dir(OwnedFd);

fn c_name(name: &OsStr) -> io::Result<CString> {
    CString::new(name.as_bytes()).map_err(|_| io::Error::other("a name with a NUL byte"))
}

fn done(result: libc::c_int) -> io::Result<()> {
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

impl Dir {
    /// Opens `path`; its last part must be a real folder. The parts before it must be the
    /// service's own.
    pub fn open(path: &Path) -> io::Result<Dir> {
        Dir::open_at(libc::AT_FDCWD, path.as_os_str())
    }

    fn open_at(at: libc::c_int, name: &OsStr) -> io::Result<Dir> {
        let flags = libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC;
        // SAFETY: the name is a valid C string for the call; a returned descriptor is ours.
        let fd = unsafe { libc::openat(at, c_name(name)?.as_ptr(), flags) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: `fd` was just opened and nothing else owns it.
        Ok(Dir(unsafe { OwnedFd::from_raw_fd(fd) }))
    }

    /// The folder `name` in this one, made (mode 0700) if missing.
    pub fn subdir(&self, name: &str, create: bool) -> io::Result<Dir> {
        if create {
            // SAFETY: a valid descriptor and C string for the call.
            let made = done(unsafe {
                libc::mkdirat(self.0.as_raw_fd(), c_name(name.as_ref())?.as_ptr(), 0o700)
            });
            match made {
                Err(e) if e.kind() != io::ErrorKind::AlreadyExists => return Err(e),
                _ => {}
            }
        }
        Dir::open_at(self.0.as_raw_fd(), name.as_ref())
    }

    /// Opens the file `name` here with `flags` (never through a link, never waiting on a pipe).
    fn file(&self, name: &str, flags: libc::c_int) -> io::Result<File> {
        let flags = flags | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC;
        // SAFETY: a valid descriptor and C string for the call; a returned descriptor is ours.
        let fd = unsafe {
            libc::openat(
                self.0.as_raw_fd(),
                c_name(name.as_ref())?.as_ptr(),
                flags,
                0o600 as libc::c_uint,
            )
        };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: `fd` was just opened and nothing else owns it.
        Ok(unsafe { File::from_raw_fd(fd) })
    }

    /// The plain file `name` for reading: `None` if there is none, or it is a link, a pipe, or
    /// a file with a second name elsewhere (a link too).
    pub fn read(&self, name: &str) -> io::Result<Option<File>> {
        match self.regular(name)? {
            Some(file) if file.metadata()?.nlink() == 1 => Ok(Some(file)),
            _ => Ok(None),
        }
    }

    /// The regular file `name` for reading: `None` if there is none, or it is a link or a pipe.
    pub fn regular(&self, name: &str) -> io::Result<Option<File>> {
        let file = match self.file(name, libc::O_RDONLY) {
            Ok(file) => file,
            // `ELOOP`: the name is a link.
            Err(e)
                if e.kind() == io::ErrorKind::NotFound || e.raw_os_error() == Some(libc::ELOOP) =>
            {
                return Ok(None);
            }
            Err(e) => return Err(e),
        };
        Ok(file.metadata()?.is_file().then_some(file))
    }

    /// A new file `name`; fails if anything has that name.
    pub fn create(&self, name: &str) -> io::Result<File> {
        self.file(name, libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL)
    }

    /// Gives `from` the name `to`, both here, in place of whatever had it (a link is replaced,
    /// not followed).
    pub fn rename(&self, from: &str, to: &str) -> io::Result<()> {
        let fd = self.0.as_raw_fd();
        // SAFETY: valid descriptors and C strings for the call.
        done(unsafe {
            libc::renameat(
                fd,
                c_name(from.as_ref())?.as_ptr(),
                fd,
                c_name(to.as_ref())?.as_ptr(),
            )
        })
    }

    /// Moves `from`, a path of the service's own, in here as `name`.
    pub fn take(&self, from: &Path, name: &str) -> io::Result<()> {
        // SAFETY: valid descriptors and C strings for the call.
        done(unsafe {
            libc::renameat(
                libc::AT_FDCWD,
                c_name(from.as_os_str())?.as_ptr(),
                self.0.as_raw_fd(),
                c_name(name.as_ref())?.as_ptr(),
            )
        })
    }

    /// Moves `name` out of here to `to`, a path of the service's own; false if there was none.
    pub fn give(&self, name: &str, to: &Path) -> io::Result<bool> {
        // SAFETY: valid descriptors and C strings for the call.
        let moved = done(unsafe {
            libc::renameat(
                self.0.as_raw_fd(),
                c_name(name.as_ref())?.as_ptr(),
                libc::AT_FDCWD,
                c_name(to.as_os_str())?.as_ptr(),
            )
        });
        match moved {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(e),
        }
    }

    /// Removes the file or link `name`; false if there was none.
    pub fn remove(&self, name: &str) -> io::Result<bool> {
        // SAFETY: a valid descriptor and C string for the call.
        match done(unsafe {
            libc::unlinkat(self.0.as_raw_fd(), c_name(name.as_ref())?.as_ptr(), 0)
        }) {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(e),
        }
    }

    /// The names in this folder (without `.` and `..`), `MAX_NAMES` at most.
    // shortcut: a folder with more entries is listed in part, in no order; page the listing
    // if a bot's memory or skills ever need that many.
    pub fn names(&self) -> io::Result<Vec<String>> {
        // SAFETY: `dup` gives a descriptor of our own, which `fdopendir` takes over and
        // `closedir` closes; `readdir`'s entry is read before the next call.
        unsafe {
            let fd = libc::dup(self.0.as_raw_fd());
            if fd < 0 {
                return Err(io::Error::last_os_error());
            }
            let stream = libc::fdopendir(fd);
            if stream.is_null() {
                let error = io::Error::last_os_error();
                libc::close(fd);
                return Err(error);
            }
            // The copy shares its place in the folder with every earlier listing.
            libc::rewinddir(stream);
            let mut names = Vec::new();
            let mut failed = None;
            while names.len() < MAX_NAMES {
                *libc::__error() = 0;
                let entry = libc::readdir(stream);
                if entry.is_null() {
                    failed =
                        Some(io::Error::last_os_error()).filter(|e| e.raw_os_error() != Some(0));
                    break;
                }
                let name = CStr::from_ptr((*entry).d_name.as_ptr());
                if let Ok(name) = name.to_str()
                    && name != "."
                    && name != ".."
                {
                    names.push(name.to_owned());
                }
            }
            libc::closedir(stream);
            failed.map_or(Ok(names), Err)
        }
    }
}
