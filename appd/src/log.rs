//! The service log, `logs/appd.log`: capped in size, with a bounded number of
//! older files (`appd.1.log` is the newest of them).

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub const CAP_BYTES: u64 = 5 << 20;
pub const KEEP: usize = 3;

pub struct Log {
    dir: PathBuf,
    cap: u64,
    keep: usize,
    file: File,
    size: u64,
    /// Also point the process's stderr at the current file, so a panic message lands there.
    onto_stderr: bool,
}

impl Log {
    pub fn open(dir: &Path, cap: u64, keep: usize, onto_stderr: bool) -> io::Result<Log> {
        let (file, size) = open_current(dir, onto_stderr)?;
        Ok(Log {
            dir: dir.to_owned(),
            cap,
            keep,
            file,
            size,
            onto_stderr,
        })
    }

    /// Appends `line` and a newline, first rotating when the line would take the file over the cap.
    pub fn write_line(&mut self, line: &str) -> io::Result<()> {
        let len = line.len() as u64 + 1;
        if self.size > 0 && self.size + len > self.cap {
            self.rotate()?;
        }
        let mut bytes = Vec::with_capacity(line.len() + 1);
        bytes.extend_from_slice(line.as_bytes());
        bytes.push(b'\n');
        self.file.write_all(&bytes)?;
        self.size += len;
        Ok(())
    }

    /// `appd.log` becomes `appd.1.log`, `appd.1.log` becomes `appd.2.log`, …; the oldest is overwritten.
    fn rotate(&mut self) -> io::Result<()> {
        for i in (1..self.keep).rev() {
            match fs::rename(
                self.dir.join(format!("appd.{i}.log")),
                self.dir.join(format!("appd.{}.log", i + 1)),
            ) {
                Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
                _ => {}
            }
        }
        // `appd.log` may be gone (the user deleted it); rotation must still open a fresh one.
        match fs::rename(self.dir.join("appd.log"), self.dir.join("appd.1.log")) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
            _ => {}
        }
        (self.file, self.size) = open_current(&self.dir, self.onto_stderr)?;
        Ok(())
    }
}

fn open_current(dir: &Path, onto_stderr: bool) -> io::Result<(File, u64)> {
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(dir.join("appd.log"))?;
    if onto_stderr {
        // SAFETY: both descriptors are valid; fd 2 now refers to the same open file.
        if unsafe { libc::dup2(file.as_raw_fd(), 2) } < 0 {
            return Err(io::Error::last_os_error());
        }
    }
    let size = file.metadata()?.len();
    Ok((file, size))
}

static LOG: Mutex<Option<Log>> = Mutex::new(None);

/// From now on `log!` writes to `<dir>/appd.log`, rotated; stderr points there too.
pub fn init(dir: &Path) -> io::Result<()> {
    let log = Log::open(dir, CAP_BYTES, KEEP, true)?;
    *LOG.lock().unwrap_or_else(|e| e.into_inner()) = Some(log);
    Ok(())
}

/// One log line; to stderr until `init`.
pub fn write(line: &str) {
    let mut guard = LOG.lock().unwrap_or_else(|e| e.into_inner());
    match guard.as_mut() {
        Some(log) => {
            if let Err(e) = log.write_line(line) {
                eprintln!("{line} (log file: {e})");
            }
        }
        None => eprintln!("{line}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn the_log_rotates_at_the_cap_and_keeps_a_bounded_number_of_files() {
        let dir = std::env::temp_dir().join(format!("appd-log-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let mut log = Log::open(&dir, 100, 2, false).unwrap();
        for i in 0..50 {
            log.write_line(&format!("line {i:02} ........")).unwrap(); // 17 bytes with the newline
        }
        let mut names: Vec<String> = fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        assert_eq!(names, ["appd.1.log", "appd.2.log", "appd.log"]);
        for name in &names {
            assert!(
                fs::metadata(dir.join(name)).unwrap().len() <= 100,
                "{name} is over the cap"
            );
        }
        // Newest lines are in appd.log, the ones just before in appd.1.log.
        assert!(
            fs::read_to_string(dir.join("appd.log"))
                .unwrap()
                .ends_with("line 49 ........\n")
        );
        assert!(
            fs::read_to_string(dir.join("appd.1.log"))
                .unwrap()
                .starts_with("line 40")
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_deleted_log_file_does_not_stop_rotation() {
        let dir = std::env::temp_dir().join(format!("appd-log-deleted-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let mut log = Log::open(&dir, 100, 2, false).unwrap();
        log.write_line("before the user deleted the file").unwrap();
        fs::remove_file(dir.join("appd.log")).unwrap();
        for i in 0..10 {
            log.write_line(&format!("line {i:02} ........")).unwrap();
        }
        assert!(
            fs::read_to_string(dir.join("appd.log"))
                .unwrap()
                .ends_with("line 09 ........\n")
        );
        fs::remove_dir_all(&dir).unwrap();
    }
}
