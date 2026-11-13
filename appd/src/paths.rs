//! Every on-disk location, rooted at `$APPD_HOME` or `~/Library/Application Support/Bate`.

use std::path::PathBuf;

pub use proto::paths::{home, run_dir, socket};

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
