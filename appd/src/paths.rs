//! Every on-disk location, rooted at `$APPD_HOME` or `~/Library/Application Support/Bate`.

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

pub fn socket() -> PathBuf {
    run_dir().join("appd.sock")
}

pub fn lock() -> PathBuf {
    run_dir().join("appd.lock")
}

pub fn db() -> PathBuf {
    home().join("app.db")
}

pub fn logs() -> PathBuf {
    home().join("logs")
}

/// A bot's folder: its engine's working directory.
pub fn bot_dir(bot: &str) -> PathBuf {
    home().join("bots").join(bot)
}
