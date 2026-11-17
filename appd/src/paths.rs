//! Every on-disk location, rooted at `$APPD_HOME` or `~/Library/Application Support/Bate`.

use std::path::{Path, PathBuf};

pub use proto::paths::{helper_socket, home, run_dir, socket};

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

/// The `Bate.app` this service runs from, if it does. A debug build takes it from `$BATE_BUNDLE`
/// (a test cannot run from a real app).
pub fn bundle() -> Option<PathBuf> {
    #[cfg(debug_assertions)]
    if let Some(app) = std::env::var_os("BATE_BUNDLE").filter(|a| !a.is_empty()) {
        return Some(app.into());
    }
    proto::paths::bundle()
}

/// Whether `app` is in `/Applications` or the user's `~/Applications`: the places macOS lets an
/// app start itself at login from, and where an update may replace it.
pub fn in_applications(app: &Path) -> bool {
    let user = std::env::var_os("HOME").map(|h| Path::new(&h).join("Applications"));
    app.parent() == Some(Path::new("/Applications")) || app.parent() == user.as_deref()
}
