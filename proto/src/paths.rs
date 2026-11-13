//! Where the service keeps its data and its socket, for the service and the window alike:
//! `$APPD_HOME`, else `~/Library/Application Support/Bate`.

use std::path::{Path, PathBuf};

pub fn home() -> PathBuf {
    match std::env::var_os("APPD_HOME").filter(|v| !v.is_empty()) {
        Some(dir) => PathBuf::from(dir),
        None => default_home(&PathBuf::from(
            std::env::var_os("HOME").unwrap_or_else(|| "/tmp".into()),
        )),
    }
}

pub fn default_home(user_home: &Path) -> PathBuf {
    user_home.join("Library/Application Support/Bate")
}

/// Holds the socket and the instance lock; mode 0700.
pub fn run_dir() -> PathBuf {
    home().join("run")
}

/// Where windows connect to the service.
pub fn socket() -> PathBuf {
    run_dir().join("appd.sock")
}
