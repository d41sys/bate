//! Running `appd` as a launchd LaunchAgent. launchd starts it at login and
//! restarts it after a crash (`KeepAlive {SuccessfulExit: false}`), not after
//! a clean exit, and the service's parent is launchd, never the window.
//!
//! The default data root uses `~/Library/LaunchAgents/com.oursnever.bate.appd.plist`.
//! A custom `$APPD_HOME` gets its own label (suffixed with a hash of the path)
//! and keeps its plist in `<home>/run/`, so test homes never touch the real agent.

use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use crate::{paths, sys};

pub const BASE_LABEL: &str = "com.oursnever.bate.appd";
/// Environment the registering process passes on to the service (and so to its engines).
// shortcut: `BATE_MODEL_API_KEY` is not passed, because the plist is plain text; #8 keeps keys in the service.
const PASS_ENV: &[&str] = &[
    "APPD_HOME",
    "APPD_ENGINE",
    "APPD_CHROME",
    "PATH",
    "BATE_MODEL_BASE_URL",
    "BATE_MODEL",
    "BATE_MODEL_CONTEXT",
];

fn custom_home() -> Option<PathBuf> {
    std::env::var_os("APPD_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

pub fn label() -> String {
    label_for(custom_home().as_deref())
}

fn label_for(custom_home: Option<&Path>) -> String {
    match custom_home {
        Some(home) => format!(
            "{BASE_LABEL}.{:08x}",
            fnv1a(home.as_os_str().as_encoded_bytes())
        ),
        None => BASE_LABEL.to_owned(),
    }
}

pub fn plist_path() -> PathBuf {
    let file = format!("{}.plist", label());
    if custom_home().is_some() {
        return paths::run_dir().join(file);
    }
    let user = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    user.join("Library/LaunchAgents").join(file)
}

/// The job description for running `appd` at `appd` with data in `home`.
fn render(label: &str, appd: &Path, home: &Path, env: &[(&str, String)]) -> String {
    let log = home.join("logs/appd.log");
    let env: String = env
        .iter()
        .map(|(k, v)| format!("    <key>{k}</key><string>{}</string>\n", xml(v)))
        .collect();
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>{label}</string>
  <key>ProgramArguments</key>
  <array><string>{program}</string></array>
  <key>EnvironmentVariables</key>
  <dict>
{env}  </dict>
  <key>WorkingDirectory</key><string>{home}</string>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key>
  <dict><key>SuccessfulExit</key><false/></dict>
  <key>ThrottleInterval</key><integer>5</integer>
  <key>StandardOutPath</key><string>{log}</string>
  <key>StandardErrorPath</key><string>{log}</string>
</dict>
</plist>
"#,
        label = xml(label),
        program = xml(&appd.to_string_lossy()),
        home = xml(&home.to_string_lossy()),
        log = xml(&log.to_string_lossy()),
    )
}

/// Registers the agent for `appd` with this process's environment and makes
/// sure it is running. A loaded agent whose plist matches is only started;
/// otherwise it is unloaded and loaded again. Refuses an `appd` in a
/// privacy-protected folder, which launchd could start but never run.
pub fn ensure(appd: &Path) -> io::Result<()> {
    // One spelling of the path, so `/tmp/…` and `/private/tmp/…` are the same agent.
    let appd = &fs::canonicalize(appd)?;
    if let Some(folder) = protected_folder(appd) {
        return Err(io::Error::other(format!(
            "launchd cannot run {} from ~/{folder}: macOS privacy protection blocks agents there; \
             install the app elsewhere (e.g. /Applications)",
            appd.display()
        )));
    }
    let pass: Vec<(&str, String)> = PASS_ENV
        .iter()
        .filter_map(|k| {
            std::env::var_os(k)
                .filter(|v| !v.is_empty())
                .map(|v| (*k, v.to_string_lossy().into_owned()))
        })
        .collect();
    let wanted = render(&label(), appd, &paths::home(), &pass);
    let path = plist_path();
    let target = service_target();
    let loaded = launchctl(&["print", &target]).is_ok();
    if loaded && fs::read_to_string(&path).is_ok_and(|current| current == wanted) {
        return launchctl(&["kickstart", &target]);
    }
    if loaded {
        launchctl(&["bootout", &target])?;
    }
    sys::create_private_dir(&paths::logs())?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("plist.tmp");
    fs::write(&tmp, &wanted)?;
    // launchd refuses a plist that others can write.
    fs::set_permissions(&tmp, fs::Permissions::from_mode(0o644))?;
    fs::rename(&tmp, &path)?;
    let domain = domain();
    let path = path.to_string_lossy();
    // `bootout` returns before launchd has fully let go of the label, so only then
    // is a failed `bootstrap` worth retrying.
    let mut tries = if loaded { 0 } else { 10 };
    loop {
        match launchctl(&["bootstrap", &domain, &path]) {
            Err(_) if tries < 10 => {
                tries += 1;
                std::thread::sleep(Duration::from_millis(200));
            }
            Err(e) => return Err(e),
            Ok(()) => return launchctl(&["kickstart", &target]),
        }
    }
}

/// Unloads the agent (stopping the service) and deletes its plist. Returns
/// whether there was anything to remove.
pub fn remove() -> io::Result<bool> {
    let target = service_target();
    let loaded = launchctl(&["print", &target]).is_ok();
    if loaded {
        launchctl(&["bootout", &target])?;
    }
    let had_file = match fs::remove_file(plist_path()) {
        Ok(()) => true,
        Err(e) if e.kind() == io::ErrorKind::NotFound => false,
        Err(e) => return Err(e),
    };
    Ok(loaded || had_file)
}

fn domain() -> String {
    // SAFETY: getuid cannot fail and touches no memory.
    format!("gui/{}", unsafe { libc::getuid() })
}

fn service_target() -> String {
    format!("{}/{}", domain(), label())
}

/// The privacy-protected home folder `path` is in, if any. A process launchd
/// starts has no grant for these folders, so it cannot even load an
/// executable stored in one.
fn protected_folder(path: &Path) -> Option<&'static str> {
    let home = PathBuf::from(std::env::var_os("HOME")?);
    [
        "Desktop",
        "Documents",
        "Downloads",
        "Library/Mobile Documents",
    ]
    .into_iter()
    .find(|f| path.starts_with(home.join(f)))
}

fn launchctl(args: &[&str]) -> io::Result<()> {
    let out = Command::new("/bin/launchctl")
        .args(args)
        .stdin(Stdio::null())
        .output()?;
    if out.status.success() {
        return Ok(());
    }
    let detail = String::from_utf8_lossy(if out.stderr.is_empty() {
        &out.stdout
    } else {
        &out.stderr
    });
    Err(io::Error::other(format!(
        "launchctl {}: {}",
        args[0],
        detail.trim()
    )))
}

fn xml(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(ch),
        }
    }
    out
}

fn fnv1a(bytes: &[u8]) -> u32 {
    bytes.iter().fold(0x811c_9dc5_u32, |h, b| {
        (h ^ u32::from(*b)).wrapping_mul(0x0100_0193)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_real_agent_runs_under_bates_label_with_data_in_application_support() {
        let home = paths::default_home(Path::new("/Users/ada"));
        assert_eq!(
            home,
            Path::new("/Users/ada/Library/Application Support/Bate")
        );
        let plist = render(
            &label_for(None),
            Path::new("/Applications/Bate.app/Contents/MacOS/appd"),
            &home,
            &[],
        );
        assert!(plist.contains("<key>Label</key><string>com.oursnever.bate.appd</string>"));
        assert!(plist.contains(
            "<array><string>/Applications/Bate.app/Contents/MacOS/appd</string></array>"
        ));
        assert!(plist.contains("<key>WorkingDirectory</key><string>/Users/ada/Library/Application Support/Bate</string>"));
        assert!(plist.contains("<key>StandardErrorPath</key><string>/Users/ada/Library/Application Support/Bate/logs/appd.log</string>"));
        assert!(plist.contains("<key>SuccessfulExit</key><false/>"));
    }

    #[test]
    fn a_test_home_gets_its_own_label_and_markup_is_escaped() {
        let label = label_for(Some(Path::new("/tmp/test-home")));
        assert!(
            label.starts_with("com.oursnever.bate.appd.") && label.len() == BASE_LABEL.len() + 9
        );
        let plist = render(
            &label,
            Path::new("/Apps/R&D <x>/appd"),
            Path::new("/tmp/h"),
            &[("APPD_ENGINE", "/e&e".into())],
        );
        assert!(plist.contains("<string>/Apps/R&amp;D &lt;x&gt;/appd</string>"));
        assert!(plist.contains("<key>APPD_ENGINE</key><string>/e&amp;e</string>"));
    }
}
