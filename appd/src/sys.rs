//! Thin wrappers over the libc calls the service needs.

use std::fs::{self, File, OpenOptions};
use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::net::UnixStream;
use std::path::Path;

/// Creates `dir` (and parents) and forces mode 0700 on it.
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

/// One line to stderr, which launchd sends to `logs/appd.log`.
macro_rules! log {
    ($($arg:tt)*) => { eprintln!("{} {}", $crate::sys::now_ms(), format_args!($($arg)*)) };
}
pub(crate) use log;
