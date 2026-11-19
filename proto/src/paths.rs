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

/// In the home: a link per bot in a project, named for the bot, to the project's folder.
pub const WORK: &str = "work";

/// Where windows connect to the service.
pub fn socket() -> PathBuf {
    run_dir().join("appd.sock")
}

/// Where the helper app connects to the service.
pub fn helper_socket() -> PathBuf {
    run_dir().join("helper.sock")
}

/// The `.app` this program runs from (it is in the app's `Contents/MacOS`), if it runs from one.
pub fn bundle() -> Option<PathBuf> {
    bundle_of(&std::env::current_exe().ok()?)
}

fn bundle_of(exe: &Path) -> Option<PathBuf> {
    let app = exe.parent()?.parent()?.parent()?;
    let in_place = exe.parent()?.ends_with("Contents/MacOS") && app.extension()? == "app";
    in_place.then(|| app.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_program_in_contents_macos_of_an_app_is_bundled() {
        let of = |p: &str| bundle_of(Path::new(p));
        assert_eq!(
            of("/Applications/Bate.app/Contents/MacOS/appd"),
            Some("/Applications/Bate.app".into())
        );
        assert_eq!(of("/x/target/release/appd"), None);
        assert_eq!(of("/x/Bate.app/Contents/Resources/appd"), None);
        assert_eq!(of("/x/Bate/Contents/MacOS/appd"), None);
        assert_eq!(of("appd"), None);
    }
}
