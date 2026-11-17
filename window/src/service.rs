//! Starts the service with the app. From `Bate.app` the app's own `appd` is asked to register
//! itself as the app's login item (`appd launch`); macOS may want the user's approval first, and
//! a failure has a reason, so the page is given a note to show while the service is not there.

use std::process::{Command, Output};
use std::sync::{Arc, Mutex};
use std::thread;

/// What `appd launch` said that the user must read, once it is done.
#[derive(Clone, Default)]
pub struct Note(Arc<Mutex<Option<String>>>);

impl Note {
    pub fn get(&self) -> Option<String> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Runs `appd launch` of the app this program is in, if it is in one, on a thread of its own.
    pub fn start(&self) {
        let Some(app) = proto::paths::bundle() else {
            return;
        };
        let note = self.0.clone();
        thread::spawn(move || {
            let out = Command::new(app.join("Contents/MacOS/appd"))
                .arg("launch")
                .output();
            let said = match out {
                Ok(out) => said(&out),
                Err(e) => Some(format!("Bate's service could not be started: {e}")),
            };
            *note.lock().unwrap_or_else(|e| e.into_inner()) = said;
        });
    }
}

/// The note for the page: the approval `appd launch` asks for, or why it failed. `None`: the
/// service is registered.
fn said(out: &Output) -> Option<String> {
    let stdout = String::from_utf8_lossy(&out.stdout);
    let first = stdout.lines().next().unwrap_or_default();
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let why = stderr
            .lines()
            .last()
            .unwrap_or("it stopped without a reason");
        return Some(format!("Bate's service could not be started: {why}"));
    }
    first
        .strip_prefix("requires approval: ")
        .map(|what| format!("macOS needs your approval: {what}."))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::process::ExitStatusExt;
    use std::process::ExitStatus;

    fn out(code: i32, stdout: &str, stderr: &str) -> Output {
        Output {
            status: ExitStatus::from_raw(code << 8),
            stdout: stdout.into(),
            stderr: stderr.into(),
        }
    }

    #[test]
    fn the_user_is_told_of_an_approval_or_a_failure_and_of_nothing_else() {
        assert_eq!(said(&out(0, "enabled\n", "")), None);
        assert_eq!(
            said(&out(
                0,
                "requires approval: switch on Bate in Login Items\n",
                ""
            )),
            Some("macOS needs your approval: switch on Bate in Login Items.".into())
        );
        let failed = said(&out(1, "", "log line\nappd: move it to /Applications\n")).unwrap();
        assert!(
            failed.ends_with("appd: move it to /Applications"),
            "{failed}"
        );
    }
}
