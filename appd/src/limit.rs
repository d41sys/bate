//! The folder limit: a bot's engine, and everything it starts, is kept inside the bot's folder
//! and the folder of its project by the operating system (macOS `sandbox-exec`), not by the
//! engine's good behaviour.

use crate::{paths, sys};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::{fs, io};

/// Everything is allowed except what is named here. Later rules win.
/// - Writing: only inside the bot's folder and its project's folder.
/// - Reading: not the users' files, other disks, temporary files, logs, Homebrew's data, other
///   terminals' input, or the service's own data (other bots, stored connector secrets); the
///   bot's folder, project folder, engine program and the one more folder named for the run
///   (a workplace's) are let through again.
/// - No way round through another program: no Apple events, no opening apps, no launchd jobs,
///   no signals to or looks at processes outside, no drivers, no shared memory, no preferences (the preferences service writes into the
///   user's home for whoever asks: an app's startup settings are a way out), and only the
///   system services a command-line program needs to look up names and check certificates
///   (so not the keychain, the clipboard or the file index).
/// - No local sockets outside the bot's folder but the system's name lookup and log. The
///   project folder does not gain socket access. Connecting is not reading: the service's own
///   socket would take the engine for a window and lift the limit on request, and the user's
///   key agent would sign for it.
/// - The network stays open unless the command is given an `Egress`: the engine has to reach its
///   model.
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
(allow file-read* (subpath (param "READ")))
(allow file-read* file-write* (subpath (param "FOLDER")))
(allow file-read* file-write* (subpath (param "WORK")))
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

/// Added for a process held to its hosts: no outgoing connection but to the service's proxy (and
/// the service's own port, for an engine), and to unix sockets in its folder. Names are not looked
/// up (that goes through a socket too): the proxy does it.
const NETWORK: &str = r#"
(deny network-outbound)
(allow network-outbound (remote unix-socket (path-literal "/private/var/run/syslog")))
(allow network-outbound (remote unix-socket (subpath (param "FOLDER"))))
(allow network-outbound (remote ip (string-append "localhost:" (param "PROXY"))))
(allow network-outbound (remote ip (string-append "localhost:" (param "SERVICE"))))
"#;

/// Where a limited process may connect, and how it is told to use it.
pub struct Egress<'a> {
    /// The service's proxy (`egress`).
    pub proxy: u16,
    /// The service's own port, for an engine's tools; a program has no use for it.
    pub service: Option<u16>,
    /// The proxy's user name for this process: the service says which hosts it may reach.
    pub key: &'a str,
}

/// The model server's key: no engine gets it in its environment. The service hands it over for
/// each model call.
const KEY: &str = "BATE_MODEL_API_KEY";

/// What a limited engine gets of the service's environment, besides its own `BATE_` settings:
/// the rest may hold the user's tokens.
const PASSED: [&str; 7] = ["PATH", "LANG", "LC_ALL", "USER", "LOGNAME", "SHELL", "TERM"];

/// Where the engine is told its working folder.
const WORK: &str = "BATE_WORK_DIR";

/// The command that starts engine `program` in `folder`, limited to it and to `work` (the
/// folder of the bot's project, or `folder` again) unless `whole_disk`. `read`: one more
/// folder it may read, which the service has checked (a workplace's). With `egress` it reaches
/// no host but through the service's proxy.
// shortcut: macOS only; elsewhere a limited bot's engine does not start. Add the system's own
// mechanism (Landlock on Linux) when the service runs there.
pub fn command(
    program: &Path,
    folder: &Path,
    work: &Path,
    read: Option<&Path>,
    whole_disk: bool,
    egress: Option<&Egress>,
) -> io::Result<Command> {
    if whole_disk {
        if egress.is_some() {
            return Err(io::Error::other(
                "the network limit is the system's: not with the whole disk",
            ));
        }
        let mut command = Command::new(program);
        command.current_dir(folder).env_remove(KEY).env(WORK, work);
        return Ok(command);
    }
    // The system compares real paths: a link in any of these would make the rule miss. `work`
    // was checked and canonicalized by the service before this call.
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
    // Without one, the program again: the sandbox needs a value, and that is let through anyway.
    let read = read.map_or(Ok(program.clone()), Path::canonicalize)?;
    let mut command = Command::new("/usr/bin/sandbox-exec");
    let profile = match egress {
        Some(_) => [PROFILE, NETWORK].concat(),
        None => PROFILE.to_owned(),
    };
    command
        .args(["-p", &profile, "-D"])
        .arg(param("FOLDER", &folder))
        .arg("-D")
        .arg(param("APP", &app))
        .arg("-D")
        .arg(param("WORK", work))
        .arg("-D")
        .arg(param("READ", &read))
        .arg("-D")
        .arg(param("ENGINE", &program))
        .env_clear()
        .envs(std::env::vars_os().filter(|(name, _)| {
            name.to_str().is_some_and(|name| {
                PASSED.contains(&name) || name.starts_with("BATE_") && name != KEY
            })
        }))
        // Programs keep their settings, caches and temporary files where they may write.
        .env("HOME", &folder)
        .env("TMPDIR", &tmp)
        .env(WORK, work)
        .current_dir(&folder);
    if let Some(e) = egress {
        // Without a service port, the proxy's again: the sandbox needs a value.
        let ports = [
            ("PROXY", e.proxy),
            ("SERVICE", e.service.unwrap_or(e.proxy)),
        ];
        for (name, port) in ports {
            command.arg("-D").arg(format!("{name}={port}"));
        }
        let proxy = format!("http://{}:x@127.0.0.1:{}", e.key, e.proxy);
        for name in ["HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY"] {
            command.env(name, &proxy).env(name.to_lowercase(), &proxy);
        }
    }
    command.arg(&program);
    Ok(command)
}

/// The folder a project may have, from the path the user chose. The project's bots read and
/// write all of it, so it is not the user's home or a folder above it, and it neither holds
/// the service's own data nor lies in it (but for `projects/`, where the service makes one).
pub fn project_folder(chosen: &str) -> Result<PathBuf, String> {
    let user = std::env::var_os("HOME").map(PathBuf::from);
    let chosen = match (chosen.strip_prefix("~/"), &user) {
        (Some(rest), Some(home)) => home.join(rest),
        _ => PathBuf::from(chosen),
    };
    if !chosen.is_absolute() {
        return Err("give the folder's full path".into());
    }
    // The real path is what the limit is set on, and what is kept.
    let folder = chosen
        .canonicalize()
        .map_err(|e| format!("{}: {e}", chosen.display()))?;
    if !folder.is_dir() {
        return Err(format!("{} is not a folder", folder.display()));
    }
    let real = |path: PathBuf| path.canonicalize().unwrap_or(path);
    match refused(&folder, user.map(real).as_deref(), &real(paths::home())) {
        Some(why) => Err(why.into()),
        None => Ok(folder),
    }
}

/// Why `folder` cannot be a project's, for a user whose home is `user` and a service whose
/// data is in `app`.
fn refused(folder: &Path, user: Option<&Path>, app: &Path) -> Option<&'static str> {
    if folder.parent().is_none() || user.is_some_and(|home| home.starts_with(folder)) {
        Some("your home folder, or a folder above it, cannot be a project's folder")
    } else if app.starts_with(folder) {
        Some("that folder holds Bate's own data")
    } else if folder.starts_with(app) && folder.parent() != Some(&app.join(paths::PROJECTS)) {
        Some("a folder inside Bate's own data cannot be a project's folder")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_project_folder_is_not_the_home_or_above_it_or_among_the_services_data() {
        let user = Path::new("/Users/u");
        let app = user.join("Library/Application Support/Bate");
        let refused = |folder: &Path| refused(folder, Some(user), &app).is_some();
        for bad in [
            Path::new("/"),
            Path::new("/Users"),
            user,
            &user.join("Library"),
            &app,
            &app.join("bots/b1"),
            &app.join("projects"),
            &app.join("projects/p1/inner"),
        ] {
            assert!(refused(bad), "{}", bad.display());
        }
        for good in [
            &user.join("code/site"),
            Path::new("/Users/other"),
            Path::new("/Volumes/disk/work"),
            &app.join("projects/p1"),
        ] {
            assert!(!refused(good), "{}", good.display());
        }
        // With no home known, the whole disk is still refused.
        assert!(super::refused(Path::new("/"), None, &app).is_some());
    }
}
