//! The folder limit: a bot's engine, and everything it starts, is kept inside the bot's folder
//! by the operating system (macOS `sandbox-exec`), not by the engine's good behaviour.

use crate::{paths, sys};
use std::ffi::OsString;
use std::path::Path;
use std::process::Command;
use std::{fs, io};

/// Everything is allowed except what is named here. Later rules win.
/// - Writing: only inside the folder.
/// - Reading: not the users' files, other disks, temporary files, logs, Homebrew's data, other
///   terminals' input, or the service's own data (other bots, stored connector secrets); the folder and
///   the engine's program are let through again.
/// - No way round through another program: no Apple events, no opening apps, no launchd jobs,
///   no signals to or looks at processes outside, no drivers, no shared memory, no preferences (the preferences service writes into the
///   user's home for whoever asks: an app's startup settings are a way out), and only the
///   system services a command-line program needs to look up names and check certificates
///   (so not the keychain, the clipboard or the file index).
/// - No local sockets outside the folder but the system's name lookup and log. Connecting is
///   not reading: the service's own socket would take the engine for a window and lift the
///   limit on request, and the user's key agent would sign for it.
/// - The network stays open: the engine has to reach its model.
// The two socket rules stay apart: a `subpath` beside `path-literal`s in one rule matches nothing.
const PROFILE: &str = r#"(version 1)
(allow default)
(deny file-write*)
(deny file-read* (subpath "/Users") (subpath "/Volumes") (subpath "/private/var/folders")
  (subpath "/private/tmp") (subpath "/cores") (subpath "/opt/homebrew/var")
  (subpath "/usr/local/var") (subpath "/private/var/log")
  (subpath "/private/var/db/diagnostics") (subpath (param "APP")))
(deny file-read-data (regex #"^/dev/ttys[0-9]+$"))
(allow file-read-metadata)
(allow file-read* (literal (param "ENGINE")))
(allow file-read* file-write* (subpath (param "FOLDER")))
(allow file-write* (literal "/dev/null") (literal "/dev/tty") (literal "/dev/dtracehelper"))
(deny appleevent-send lsopen job-creation)
(deny user-preference*)
(deny signal process-info*)
(allow signal process-info* (target same-sandbox))
(deny iokit-open ipc-posix-shm*)
(allow ipc-posix-shm-read* (ipc-posix-name "apple.shm.notification_center"))
(deny mach-lookup)
(allow mach-lookup
  (global-name "com.apple.system.opendirectoryd.libinfo")
  (global-name "com.apple.system.opendirectoryd.membership")
  (global-name "com.apple.system.notification_center")
  (global-name "com.apple.system.logger")
  (global-name "com.apple.logd")
  (global-name "com.apple.trustd.agent")
  (global-name "com.apple.trustd")
  (global-name "com.apple.SystemConfiguration.configd")
  (global-name "com.apple.SystemConfiguration.DNSConfiguration")
  (global-name "com.apple.dnssd.service")
  (global-name "com.apple.bsd.dirhelper")
  (global-name "com.apple.networkd")
  (global-name "com.apple.nehelper")
  (global-name "com.apple.symptomsd")
  (global-name "com.apple.usymptomsd"))
(deny network-outbound (remote unix-socket))
(allow network-outbound (remote unix-socket
  (path-literal "/private/var/run/mDNSResponder") (path-literal "/private/var/run/syslog")))
(allow network-outbound (remote unix-socket (subpath (param "FOLDER"))))
"#;

/// What a limited engine gets of the service's environment, besides its own `BATE_` settings:
/// the rest may hold the user's tokens.
const PASSED: [&str; 7] = ["PATH", "LANG", "LC_ALL", "USER", "LOGNAME", "SHELL", "TERM"];

/// The command that starts engine `program` in `folder`, limited to it unless `whole_disk`.
// shortcut: macOS only; elsewhere a limited bot's engine does not start. Add the system's own
// mechanism (Landlock on Linux) when the service runs there.
pub fn command(program: &Path, folder: &Path, whole_disk: bool) -> io::Result<Command> {
    if whole_disk {
        let mut command = Command::new(program);
        command.current_dir(folder);
        return Ok(command);
    }
    // The system compares real paths: a link in any of these would make the rule miss.
    let (program, folder) = (program.canonicalize()?, folder.canonicalize()?);
    let app = paths::home().canonicalize()?;
    let tmp = folder.join(".tmp");
    // The bot may have left a link there; the service must not follow it out of the folder.
    if fs::symlink_metadata(&tmp).is_ok_and(|meta| !meta.is_dir()) {
        fs::remove_file(&tmp)?;
    }
    sys::create_private_dir(&tmp)?;
    let param = |name: &str, path: &Path| {
        let mut param = OsString::from(format!("{name}="));
        param.push(path);
        param
    };
    let mut command = Command::new("/usr/bin/sandbox-exec");
    command
        .args(["-p", PROFILE, "-D"])
        .arg(param("FOLDER", &folder))
        .arg("-D")
        .arg(param("APP", &app))
        .arg("-D")
        .arg(param("ENGINE", &program))
        .arg(&program)
        .env_clear()
        .envs(std::env::vars_os().filter(|(name, _)| {
            name.to_str()
                .is_some_and(|name| PASSED.contains(&name) || name.starts_with("BATE_"))
        }))
        // Programs keep their settings, caches and temporary files where they may write.
        .env("HOME", &folder)
        .env("TMPDIR", &tmp)
        .current_dir(&folder);
    Ok(command)
}
