//! Updates from a static feed: a JSON file that names a release archive, its SHA-256 and an
//! Ed25519 signature. The service takes nothing that fails the hash, the signature or the
//! version rule, and never swaps a program while a bot is replying (the core decides when).

use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use ring::digest::{Context, SHA256};
use ring::rand::SystemRandom;
use ring::signature::{ED25519, Ed25519KeyPair, KeyPair, UnparsedPublicKey};
use serde::{Deserialize, Serialize};

use crate::{attach, oauth, paths};

const DEFAULT_FEED: &str = "https://github.com/d41sys/bate/releases/latest/download/feed.json";
const RELEASES: &str = "https://github.com/d41sys/bate/releases/download";
const MAX_FEED: u64 = 64 << 10;
const MAX_ARCHIVE: u64 = 300 << 20;
/// The app a release archive holds, and where its programs are in it.
const APP: &str = "Bate.app";
const MACOS: &str = "Contents/MacOS";
/// The programs the service swaps when it does not run from an app. `agentcore` goes first: a
/// half-done swap is then a new engine under the old service, which the service already
/// copes with, not the reverse.
const PROGRAMS: [&str; 2] = ["agentcore", "appd"];
/// How often the feed is read.
pub const EVERY: Duration = Duration::from_secs(24 * 3600);
/// The service's exit status after a swap. Not zero: launchd restarts a service only when it
/// exits that way (`KeepAlive {SuccessfulExit: false}`).
pub const RESTART_CODE: i32 = 75;

/// What the feed says about the newest release.
#[derive(Clone, Serialize, Deserialize)]
pub struct Feed {
    pub version: String,
    pub url: String,
    pub sha256: String,
    pub signature: String,
}

/// A downloaded, verified and unpacked release.
pub struct Staged {
    pub version: String,
    dir: PathBuf,
}

pub fn current() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// The key releases are signed with: compiled in, so that a Mac cannot be told to trust
/// another. Without one, updates are off. A debug build (the tests) reads it from the
/// environment instead, since a test needs its own throwaway key.
pub fn public_key() -> Option<Vec<u8>> {
    let compiled = option_env!("BATE_UPDATE_PUBKEY");
    #[cfg(debug_assertions)]
    let key = std::env::var("BATE_UPDATE_PUBKEY")
        .ok()
        .or(compiled.map(str::to_owned));
    #[cfg(not(debug_assertions))]
    let key = compiled.map(str::to_owned);
    key.and_then(|k| attach::base64(k.trim()))
        .filter(|k| k.len() == 32)
}

/// Where the new programs go: next to this one, unless a test says otherwise.
fn install_dir() -> io::Result<PathBuf> {
    if let Some(dir) = std::env::var_os("BATE_UPDATE_INSTALL_DIR").filter(|d| !d.is_empty()) {
        return Ok(dir.into());
    }
    let exe = std::env::current_exe()?;
    exe.parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| io::Error::other("this program has no folder"))
}

/// `a.b.c`, three whole numbers: anything else is never newer than anything.
fn parse_version(v: &str) -> Option<[u64; 3]> {
    let mut parts = v.split('.').map(|p| {
        let digits = !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit());
        digits.then(|| p.parse::<u64>().ok()).flatten()
    });
    let version = [parts.next()??, parts.next()??, parts.next()??];
    parts.next().is_none().then_some(version)
}

/// The bytes that are signed. The version is in them, so that an old signed archive cannot be
/// passed off as a new release.
fn message(version: &str, sha256: &str) -> String {
    format!("bate-update\n{version}\n{sha256}")
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Standard padded base64.
fn base64(bytes: &[u8]) -> String {
    let mut out = oauth::base64url(bytes).replace('-', "+").replace('_', "/");
    while !out.len().is_multiple_of(4) {
        out.push('=');
    }
    out
}

/// Whether `feed` offers a release newer than `running`; an error says why it is not to be
/// trusted, or is older.
fn judge(feed: &Feed, key: &[u8], running: &str) -> Result<bool, String> {
    let (Some(offered), Some(have)) = (parse_version(&feed.version), parse_version(running)) else {
        return Err(format!(
            "the feed's version {:?} is not of the form 1.2.3",
            feed.version
        ));
    };
    let sha_ok = feed.sha256.len() == 64
        && feed
            .sha256
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    if !sha_ok {
        return Err("the feed's sha256 is not 64 lowercase hex digits".into());
    }
    let signature = attach::base64(&feed.signature).ok_or("the feed's signature is not base64")?;
    UnparsedPublicKey::new(&ED25519, key)
        .verify(message(&feed.version, &feed.sha256).as_bytes(), &signature)
        .map_err(|_| "the feed's signature does not match: not signed by Bate's key".to_owned())?;
    if offered < have {
        return Err(format!(
            "the feed offers {}, which is older than the running {running}",
            feed.version
        ));
    }
    Ok(offered > have)
}

fn agent() -> ureq::Agent {
    // Redirects are followed: GitHub serves release files from another host. Nothing secret is sent.
    let config = ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(15)))
        .timeout_global(Some(Duration::from_secs(600)))
        .build();
    ureq::Agent::new_with_config(config)
}

/// Reads the feed; when it offers a newer release than the running one (and than `staged`),
/// downloads, checks and unpacks it under `updates/`. `None`: nothing new.
pub fn fetch(key: &[u8], staged: Option<&str>) -> Result<Option<Staged>, String> {
    let url = std::env::var("BATE_UPDATE_FEED")
        .ok()
        .filter(|u| !u.is_empty())
        .unwrap_or_else(|| DEFAULT_FEED.to_owned());
    let agent = agent();
    let mut body = agent
        .get(&url)
        .call()
        .map_err(|e| format!("cannot read the feed {url}: {e}"))?
        .into_body();
    let text = body
        .with_config()
        .limit(MAX_FEED)
        .read_to_vec()
        .map_err(|e| format!("cannot read the feed {url}: {e}"))?;
    let feed: Feed = serde_json::from_slice(&text).map_err(|e| format!("bad feed: {e}"))?;
    if !judge(&feed, key, current())? || staged == Some(feed.version.as_str()) {
        return Ok(None);
    }
    let updates = paths::home().join("updates");
    let dir = updates.join(&feed.version);
    // Only the release that waits to be installed is kept.
    if let Ok(old) = fs::read_dir(&updates) {
        for entry in old.flatten() {
            if staged.is_none_or(|s| entry.file_name() != s) {
                let _ = fs::remove_dir_all(entry.path()).or_else(|_| fs::remove_file(entry.path()));
            }
        }
    }
    let _ = fs::remove_dir_all(&dir);
    crate::sys::create_private_dir(&dir).map_err(|e| e.to_string())?;
    stage(&agent, &feed, &dir).inspect_err(|_| {
        let _ = fs::remove_dir_all(&dir);
    })?;
    Ok(Some(Staged {
        version: feed.version,
        dir,
    }))
}

/// Downloads the archive to `dir`, checks its hash, and unpacks the programs there.
fn stage(agent: &ureq::Agent, feed: &Feed, dir: &Path) -> Result<(), String> {
    let err = |what: &str, e: &dyn std::fmt::Display| format!("{what}: {e}");
    let archive = dir.join("release.tar.gz");
    let mut body = agent
        .get(&feed.url)
        .call()
        .map_err(|e| err(&format!("cannot download {}", feed.url), &e))?
        .into_body();
    let mut file = File::create(&archive).map_err(|e| err("cannot write the download", &e))?;
    let (mut hash, mut buf, mut size) = (Context::new(&SHA256), [0u8; 64 << 10], 0u64);
    let mut reader = body.as_reader();
    loop {
        let n = reader
            .read(&mut buf)
            .map_err(|e| err("the download broke off", &e))?;
        if n == 0 {
            break;
        }
        size += n as u64;
        if size > MAX_ARCHIVE {
            return Err("the download is larger than any release can be".into());
        }
        hash.update(&buf[..n]);
        file.write_all(&buf[..n])
            .map_err(|e| err("cannot write the download", &e))?;
    }
    let got = hex(hash.finish().as_ref());
    if got != feed.sha256 {
        return Err(format!(
            "the download's sha256 is {got}, not the feed's {}",
            feed.sha256
        ));
    }
    // Only the app is named, so nothing else in the archive is written.
    let out = Command::new("/usr/bin/tar")
        .arg("-xzf")
        .arg(&archive)
        .arg("-C")
        .arg(dir)
        .arg(APP)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| err("cannot run tar", &e))?;
    if !out.status.success() {
        return Err(format!(
            "cannot unpack the release: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    for name in PROGRAMS {
        let plain =
            fs::symlink_metadata(dir.join(APP).join(MACOS).join(name)).is_ok_and(|m| m.is_file());
        if !plain {
            return Err(format!("the release has no program {name}"));
        }
    }
    let _ = fs::remove_file(&archive);
    Ok(())
}

/// Puts the staged release in place. From a `Bate.app`, the whole app is replaced: a program
/// swapped inside a signed app would break its seal. Otherwise (a build folder) the two
/// programs are, each copied next to its target first and then renamed over it, so a program
/// is never half written.
pub fn install(staged: &Staged) -> io::Result<()> {
    let new = staged.dir.join(APP);
    if let Some(app) = paths::bundle() {
        replace_app(&new, &app)?;
        // macOS cannot start a changed app's login item until it is registered again (see
        // `login_item.rs`), so the service would not come back after its exit: the new app
        // registers it, and says in the service's log if that fails. It must outlive this
        // service (its own process group; the job's plist has `AbandonProcessGroup`).
        let log = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(paths::logs().join("appd.log"));
        let mut relaunch = Command::new(app.join(MACOS).join("appd"));
        relaunch.arg("launch").process_group(0).stdin(Stdio::null());
        if let Ok(log) = log {
            relaunch.stdout(log.try_clone()?).stderr(log);
        }
        let _ = relaunch.spawn();
        return Ok(());
    }
    let (from, target) = (new.join(MACOS), install_dir()?);
    for name in PROGRAMS {
        let tmp = target.join(format!(".{name}.update"));
        fs::copy(from.join(name), &tmp)?;
        fs::set_permissions(&tmp, fs::Permissions::from_mode(0o755))?;
    }
    for name in PROGRAMS {
        fs::rename(target.join(format!(".{name}.update")), target.join(name))?;
    }
    Ok(())
}

/// Puts a copy of `new` where `app` is, in one step: the copy is made next to `app` and then
/// exchanged with it, so `app` is always a whole app, the old or the new.
// shortcut: the window keeps running its old code until it is opened again; only the service restarts.
fn replace_app(new: &Path, app: &Path) -> io::Result<()> {
    let parent = app
        .parent()
        .ok_or_else(|| io::Error::other("the app has no folder"))?;
    let copy = parent.join(format!(".{APP}.update"));
    let _ = fs::remove_dir_all(&copy);
    // `ditto` keeps the links, modes and extended attributes a signature covers.
    let status = Command::new("/usr/bin/ditto")
        .arg(new)
        .arg(&copy)
        .status()?;
    if !status.success() {
        let _ = fs::remove_dir_all(&copy);
        return Err(io::Error::other(format!(
            "cannot copy the new app to {}",
            copy.display()
        )));
    }
    let swapped = exchange(&copy, app);
    // What is left at `copy` is the old app after a swap, the unused copy after a failure.
    let _ = fs::remove_dir_all(&copy);
    swapped
}

/// Exchanges two paths atomically (`renamex_np` with `RENAME_SWAP`).
fn exchange(a: &Path, b: &Path) -> io::Result<()> {
    let c = |p: &Path| std::ffi::CString::new(p.as_os_str().as_encoded_bytes());
    let (a, b) = (c(a)?, c(b)?);
    // SAFETY: two valid NUL-terminated paths.
    let rc = unsafe { libc::renamex_np(a.as_ptr(), b.as_ptr(), libc::RENAME_SWAP) };
    if rc == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// `appd update keygen`: a new signing key for releases.
pub fn keygen() -> io::Result<()> {
    let pkcs8 = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new())
        .map_err(|_| io::Error::other("no random numbers"))?;
    let pair =
        Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).map_err(|e| io::Error::other(e.to_string()))?;
    println!("BATE_UPDATE_KEY={}", base64(pkcs8.as_ref()));
    println!("BATE_UPDATE_PUBKEY={}", base64(pair.public_key().as_ref()));
    Ok(())
}

/// `appd update sign <archive> <version> [url]`: prints the feed for `archive`, signed with
/// the key in `$BATE_UPDATE_KEY`. `url` is where the archive will be: by default its place in
/// the GitHub release `v<version>`.
pub fn sign(
    archive: Option<String>,
    version: Option<String>,
    url: Option<String>,
) -> io::Result<()> {
    let (Some(archive), Some(version)) = (archive, version) else {
        return Err(io::Error::other(
            "usage: appd update sign <archive> <version> [url]",
        ));
    };
    if parse_version(&version).is_none() {
        return Err(io::Error::other("the version must be of the form 1.2.3"));
    }
    let key = std::env::var("BATE_UPDATE_KEY")
        .ok()
        .and_then(|k| attach::base64(k.trim()))
        .ok_or_else(|| io::Error::other("BATE_UPDATE_KEY must hold the base64 private key"))?;
    let pair = Ed25519KeyPair::from_pkcs8(&key)
        .map_err(|e| io::Error::other(format!("BATE_UPDATE_KEY is not a private key: {e}")))?;
    let sha256 = hex(ring::digest::digest(&SHA256, &fs::read(&archive)?).as_ref());
    let url = url.unwrap_or_else(|| {
        let name = Path::new(&archive).file_name().unwrap_or_default();
        format!("{RELEASES}/v{version}/{}", name.to_string_lossy())
    });
    let signature = base64(pair.sign(message(&version, &sha256).as_bytes()).as_ref());
    let feed = Feed {
        version,
        url,
        sha256,
        signature,
    };
    println!("{}", serde_json::to_string_pretty(&feed)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_are_three_numbers_and_compare_by_number() {
        assert_eq!(parse_version("0.10.2"), Some([0, 10, 2]));
        assert!(parse_version("0.10.2") > parse_version("0.9.9"));
        for bad in [
            "1.2",
            "1.2.3.4",
            "1.2.x",
            "1..3",
            "+1.2.3",
            "v1.2.3",
            "1.2.3-rc1",
        ] {
            assert_eq!(parse_version(bad), None, "{bad}");
        }
    }

    #[test]
    fn base64_round_trips_through_the_decoder() {
        for len in 0..40 {
            let bytes: Vec<u8> = (0..len).map(|i| (i * 37 + 250) as u8).collect();
            assert_eq!(attach::base64(&base64(&bytes)).unwrap(), bytes);
        }
    }
}
