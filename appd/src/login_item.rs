//! The service as a login item of `Bate.app`, through macOS's `SMAppService`: the app carries the
//! job description in `Contents/Library/LaunchAgents/`, and macOS runs the app's own `appd` from
//! it at login and after a crash. Nothing is written to `~/Library/LaunchAgents`, so the service
//! shows in System Settings under the app's name, and replacing the app replaces the service.
//! A service run from a build folder keeps `launch.rs`.

use std::fs;
use std::io;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::{launch, paths, sys};

/// The login item's label: `launch::BASE_LABEL`, which names its job description,
/// `<label>.plist`, in `Contents/Library/LaunchAgents`. A debug build takes another from
/// `$BATE_LOGIN_ITEM_ID`, so that a trial of the real thing can use an app of its own.
fn id() -> String {
    #[cfg(debug_assertions)]
    if let Some(id) = std::env::var_os("BATE_LOGIN_ITEM_ID").filter(|i| !i.is_empty()) {
        return id.to_string_lossy().into_owned();
    }
    launch::BASE_LABEL.to_owned()
}

/// Whether `$BATE_LOGIN_ITEM_ID` names an id of its own (debug builds).
pub fn custom_id() -> bool {
    id() != launch::BASE_LABEL
}

fn plist() -> String {
    format!("{}.plist", id())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Status {
    NotRegistered,
    Enabled,
    RequiresApproval,
    NotFound,
}

const APPROVAL: &str = "requires approval: switch on Bate in System Settings > General > Login Items & Extensions, then open Bate again";

/// How many times a registration is tried before it is called failed, how long macOS is given
/// to start the service after each, and the pause before registering again.
const TRIES: u32 = 6;
const START: Duration = Duration::from_secs(10);
const PAUSE: Duration = Duration::from_secs(3);

/// `appd launch` from `app`: registers its service, and says where that stands: `enabled`, or
/// `requires approval: …`, in which case the pane where the user gives it opens (once). A service
/// registered for this very build is left alone.
///
/// macOS cannot start the login item of an app that changed (an update replaces the whole app)
/// until it is registered again, and goes on calling it `enabled`; a registration made too soon
/// after the change is lost too (measured: the first one fails, a second one, a few seconds
/// later, takes). So the build is remembered in `run/login-item`, a registration is followed by
/// a look at whether launchd really started the service, and it is made again if not.
pub fn start(app: &Path) -> io::Result<String> {
    if !paths::in_applications(app) {
        return Err(io::Error::other(format!(
            "macOS starts a bundle's service only from /Applications or ~/Applications; move {} there and open it again",
            app.display()
        )));
    }
    sys::create_private_dir(&paths::run_dir())?;
    // One registration at a time: the window and an update may both ask.
    let Some(_lock) = sys::try_lock(&paths::run_dir().join("login-item.lock"))? else {
        return Ok("enabled".into());
    };
    let build = build_of(app)?;
    let marker = paths::run_dir().join("login-item");
    let known = fs::read_to_string(&marker).is_ok_and(|seen| seen == build);
    if known {
        match status() {
            Status::Enabled => return Ok("enabled".into()),
            Status::RequiresApproval => return Ok(APPROVAL.into()),
            Status::NotRegistered | Status::NotFound => {}
        }
    }
    for _ in 0..TRIES {
        let _ = unregister();
        pause(PAUSE);
        register()?;
        match status() {
            Status::Enabled if started() => {
                fs::write(&marker, build)?;
                return Ok("enabled".into());
            }
            Status::Enabled => {}
            Status::RequiresApproval => {
                open_settings();
                fs::write(&marker, build)?;
                return Ok(APPROVAL.into());
            }
            other => {
                return Err(io::Error::other(format!(
                    "macOS did not take the service ({other:?}): is {} in {}/Contents/Library/LaunchAgents?",
                    plist(),
                    app.display()
                )));
            }
        }
    }
    Err(io::Error::other(
        "macOS took the service but did not start it; open Bate again, or switch Bate off and on in System Settings > General > Login Items & Extensions",
    ))
}

/// Waits for launchd to start the service: `true` once it runs (or has run to a clean end: a
/// copy of `appd` that was already serving).
fn started() -> bool {
    #[cfg(debug_assertions)]
    if let Some(dir) = fake() {
        return fake_started(&dir);
    }
    let target = format!("{}/{}", launch::domain(), id());
    let deadline = Instant::now() + START;
    while Instant::now() < deadline {
        let print = Command::new("/bin/launchctl")
            .args(["print", &target])
            .stdin(Stdio::null())
            .output();
        let text = print.map(|o| String::from_utf8_lossy(&o.stdout).into_owned());
        if text.is_ok_and(|t| t.contains("state = running") || t.contains("last exit code = 0")) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    false
}

fn pause(time: Duration) {
    #[cfg(debug_assertions)]
    if fake().is_some() {
        return;
    }
    std::thread::sleep(time);
}

/// The build of `app`: the SHA-256 of its service program.
fn build_of(app: &Path) -> io::Result<String> {
    let appd = fs::read(app.join("Contents/MacOS/appd"))?;
    let sha = ring::digest::digest(&ring::digest::SHA256, &appd);
    Ok(sha.as_ref().iter().map(|b| format!("{b:02x}")).collect())
}

/// `appd unregister` from a bundle: the service stops and is no longer started at login.
pub fn stop() -> io::Result<bool> {
    let was = status() != Status::NotRegistered;
    unregister()?;
    Ok(was)
}

/// The fake a debug build uses when `$BATE_LOGIN_ITEM_FAKE` names a folder: tests must not
/// register a real service. `status` there is its state, `needs_approval` makes `register` end
/// in approval, and `calls` records what was asked.
#[cfg(debug_assertions)]
fn fake() -> Option<std::path::PathBuf> {
    std::env::var_os("BATE_LOGIN_ITEM_FAKE").map(Into::into)
}

/// The fake's `spawn_fails` holds how many looks at the service still find it not started.
#[cfg(debug_assertions)]
fn fake_started(dir: &Path) -> bool {
    let left = fs::read_to_string(dir.join("spawn_fails"))
        .ok()
        .and_then(|n| n.trim().parse::<u32>().ok())
        .unwrap_or(0);
    let _ = fs::write(dir.join("spawn_fails"), left.saturating_sub(1).to_string());
    left == 0
}

#[cfg(debug_assertions)]
fn fake_call(dir: &Path, what: &str) {
    use std::io::Write;
    let calls = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("calls"));
    let _ = calls.and_then(|mut f| writeln!(f, "{what}"));
}

fn status() -> Status {
    #[cfg(debug_assertions)]
    if let Some(dir) = fake() {
        return match fs::read_to_string(dir.join("status")).as_deref() {
            Ok("enabled") => Status::Enabled,
            Ok("requires_approval") => Status::RequiresApproval,
            Ok("not_found") => Status::NotFound,
            _ => Status::NotRegistered,
        };
    }
    // SAFETY: the calls are plain Objective-C messages on a class and an object it returns.
    let status = unsafe { service().status() };
    match status {
        SMAppServiceStatus::Enabled => Status::Enabled,
        SMAppServiceStatus::RequiresApproval => Status::RequiresApproval,
        SMAppServiceStatus::NotFound => Status::NotFound,
        _ => Status::NotRegistered,
    }
}

fn register() -> io::Result<()> {
    #[cfg(debug_assertions)]
    if let Some(dir) = fake() {
        fake_call(&dir, "register");
        let approval = dir.join("needs_approval").exists();
        let state = if approval {
            "requires_approval"
        } else {
            "enabled"
        };
        return fs::write(dir.join("status"), state);
    }
    // An agent that `appd launch` put in ~/Library/LaunchAgents before has the same label. (Only
    // that one: `launch::remove` also boots out whatever launchd has under the label, which is
    // this login item when it is already registered.)
    if !custom_id() && launch::plist_path().exists() {
        launch::remove()?;
    }
    // SAFETY: as in `status`.
    unsafe { service().registerAndReturnError() }.map_err(|e| io::Error::other(e.to_string()))
}

fn unregister() -> io::Result<()> {
    #[cfg(debug_assertions)]
    if let Some(dir) = fake() {
        fake_call(&dir, "unregister");
        return fs::write(dir.join("status"), "not_registered");
    }
    // SAFETY: as in `status`.
    unsafe { service().unregisterAndReturnError() }.map_err(|e| io::Error::other(e.to_string()))
}

fn open_settings() {
    #[cfg(debug_assertions)]
    if let Some(dir) = fake() {
        return fake_call(&dir, "settings");
    }
    // SAFETY: as in `status`.
    unsafe { SMAppService::openSystemSettingsLoginItems() };
}

use objc2_foundation::NSString;
use objc2_service_management::{SMAppService, SMAppServiceStatus};

fn service() -> objc2::rc::Retained<SMAppService> {
    // SAFETY: a class method that takes a string.
    unsafe { SMAppService::agentServiceWithPlistName(&NSString::from_str(&plist())) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bundles_job_description_is_the_services_own() {
        let plist = include_str!("../../window/bundle/com.oursnever.bate.appd.plist");
        assert!(plist.contains(&format!(
            "<key>Label</key><string>{}</string>",
            launch::BASE_LABEL
        )));
        assert!(plist.contains("<key>BundleProgram</key><string>Contents/MacOS/appd</string>"));
        // As in `launch.rs`: a clean exit stays stopped, any other one is restarted.
        assert!(plist.contains("<dict><key>SuccessfulExit</key><false/></dict>"));
        // `appd update` starts a program that must outlive the service's exit (see `update.rs`).
        assert!(plist.contains("<key>AbandonProcessGroup</key><true/>"));
        assert_eq!(plist(), format!("{}.plist", launch::BASE_LABEL));
    }
}
