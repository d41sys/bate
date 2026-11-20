//! Workplaces: installed packages whose program runs as a project's connector. A workplace is
//! a folder with a `workplace.json` (format in `docs/workplaces.md`). It is copied into
//! `workplaces/<name>/` in the service's home, which no bot or program can write, and a
//! fingerprint of the files is kept: when they change, the workplace is off until the person
//! looks.

use crate::core::check_connector_name;
use crate::download;
use crate::skills::{self, Limits};
use crate::store::Workplace;
use crate::sys::Dir;
use crate::{paths, sys};
use proto::skills::Skill;
use ring::digest;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{self, Read};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub const FILE: &str = "workplace.json";
const MAX_MANIFEST: u64 = 64 * 1024;
/// A program may come with its libraries: more than a skill, still bounded.
const LIMITS: Limits = Limits {
    files: 2000,
    bytes: 200 * 1024 * 1024,
    depth: 12,
    what: "workplace",
};
/// Most entries in a list of the manifest, and most characters of one entry.
const MOST: usize = 50;
const LONGEST: usize = 200;
const MOST_DOWNLOADS: usize = 20;
/// The most one download may be, and the stated size cannot be more.
const LARGEST_DOWNLOAD: u64 = 16 << 30;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub name: String,
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub program: Program,
    #[serde(default)]
    pub needs: Needs,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub downloads: Vec<Download>,
}

impl Manifest {
    /// What the downloads come to, the stated sizes added.
    pub fn download_bytes(&self) -> u64 {
        self.downloads.iter().map(|d| d.size).sum()
    }

    /// The hosts (with a port, if there is one) the downloads contact, each once.
    pub fn hosts(&self) -> Vec<String> {
        let mut hosts: Vec<_> = (self.downloads.iter())
            .filter_map(|d| host_of(&d.url).ok())
            .collect();
        hosts.sort();
        hosts.dedup();
        hosts
    }
}

/// A file the workplace needs that is too big to ship in its folder. Bate fetches it at
/// install, checks `size` and `sha256`, and puts it at `dest` in the workplace's folder, or,
/// with `unpack: "tar.gz"`, unpacks the archive into the folder `dest`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Download {
    pub url: String,
    pub size: u64,
    pub sha256: String,
    pub dest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unpack: Option<String>,
}

/// `command` is a path in the workplace's folder, or a name on the service's `PATH`; `args`
/// start there too.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Program {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
}

/// What the person is shown before switching the workplace on. Hosts are stored and shown,
/// not yet enforced.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Needs {
    #[serde(default)]
    pub folders: Vec<String>,
    #[serde(default)]
    pub hosts: Vec<String>,
    #[serde(default)]
    pub commands: Vec<String>,
}

/// A workplace as `list_workplaces` shows it.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Listed {
    pub name: String,
    pub version: String,
    /// `None`: the manifest is malformed, `error` says how.
    pub manifest: Option<Manifest>,
    pub skills: Vec<Skill>,
    /// Whole paths of the role files, for `read_roles`.
    pub roles: Vec<PathBuf>,
    /// The files are not what was approved.
    pub changed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// The ids of the projects it is switched on in.
    pub used_by: Vec<String>,
}

impl Listed {
    /// The manifest if the workplace may run: well-formed and as approved.
    pub fn ready(&self) -> Result<&Manifest, String> {
        if let Some(e) = &self.error {
            return Err(format!("the workplace {} is broken: {e}", self.name));
        }
        match &self.manifest {
            Some(m) if !self.changed => Ok(m),
            _ => Err(format!(
                "the files of the workplace {} changed since they were approved: look at them, then approve it again",
                self.name
            )),
        }
    }
}

pub fn dir() -> PathBuf {
    paths::home().join("workplaces")
}

pub fn folder(name: &str) -> PathBuf {
    dir().join(name)
}

fn line(what: &str, text: &str, longest: usize) -> Result<(), String> {
    if text.trim().is_empty() || text.chars().count() > longest || text.contains(char::is_control) {
        return Err(format!(
            "`{what}` must be one line of 1 to {longest} characters"
        ));
    }
    Ok(())
}

fn list_of(what: &str, items: &[String]) -> Result<(), String> {
    if items.len() > MOST {
        return Err(format!("`{what}` has at most {MOST} entries"));
    }
    items.iter().try_for_each(|item| line(what, item, LONGEST))
}

/// `host[:port]` of an address that may be downloaded from: https, or http to this Mac.
fn host_of(url: &str) -> Result<String, String> {
    let bad = || format!("`downloads.url` {url:?} must be an https address");
    let uri: ureq::http::Uri = url.parse().map_err(|_| bad())?;
    let authority = uri.authority().ok_or_else(bad)?;
    let host = authority.host().to_ascii_lowercase();
    let local = ["127.0.0.1", "localhost", "[::1]"].contains(&host.as_str());
    let scheme_ok = match uri.scheme_str() {
        Some("https") => true,
        Some("http") => local,
        _ => false,
    };
    if !scheme_ok || host.is_empty() || authority.as_str().contains('@') {
        return Err(bad());
    }
    Ok(authority.as_str().to_ascii_lowercase())
}

fn check_downloads(list: &[Download]) -> Result<(), String> {
    if list.len() > MOST_DOWNLOADS {
        return Err(format!("`downloads` has at most {MOST_DOWNLOADS} entries"));
    }
    for (i, d) in list.iter().enumerate() {
        line("downloads.url", &d.url, 2000)?;
        host_of(&d.url)?;
        if d.size == 0 || d.size > LARGEST_DOWNLOAD {
            return Err(format!("`downloads.size` is 1 to {LARGEST_DOWNLOAD} bytes"));
        }
        let hex = d
            .sha256
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
        if d.sha256.len() != 64 || !hex {
            return Err("`downloads.sha256` is 64 lowercase hex digits".into());
        }
        line("downloads.dest", &d.dest, LONGEST)?;
        if !Path::new(&d.dest)
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
        {
            return Err(
                "`downloads.dest` is a path inside the workplace's folder: not absolute, no `..`"
                    .into(),
            );
        }
        if d.unpack.as_deref().is_some_and(|u| u != "tar.gz") {
            return Err("`downloads.unpack` is `tar.gz` or left out".into());
        }
        if list[..i].iter().any(|o| o.dest == d.dest) {
            return Err(format!("`downloads.dest` {:?} is used twice", d.dest));
        }
    }
    Ok(())
}

fn parse(text: &str) -> Result<Manifest, String> {
    let m: Manifest = serde_json::from_str(text).map_err(|e| format!("{FILE}: {e}"))?;
    check_connector_name(&m.name)?;
    line("version", &m.version, 40)?;
    if let Some(description) = &m.description {
        line("description", description, 500)?;
    }
    let command = &m.program.command;
    line("program.command", command, LONGEST)?;
    // A path stays in the workplace's folder; a bare name is looked up on the PATH.
    let path = Path::new(command);
    if command.contains('/') && !path.components().all(|c| matches!(c, Component::Normal(_))) {
        return Err(
            "`program.command` is a path inside the workplace's folder: not absolute, no `..`"
                .into(),
        );
    }
    list_of("program.args", &m.program.args)?;
    list_of("needs.folders", &m.needs.folders)?;
    list_of("needs.hosts", &m.needs.hosts)?;
    list_of("needs.commands", &m.needs.commands)?;
    check_downloads(&m.downloads)?;
    Ok(m)
}

fn read(folder: &Dir) -> Result<Manifest, String> {
    let file = match folder.regular(FILE) {
        Ok(Some(file)) => file,
        Ok(None) => return Err(format!("the folder has no {FILE}")),
        Err(e) => return Err(format!("cannot read {FILE}: {e}")),
    };
    let mut text = String::new();
    file.take(MAX_MANIFEST + 1)
        .read_to_string(&mut text)
        .map_err(|e| format!("cannot read {FILE}: {e}"))?;
    if text.len() as u64 > MAX_MANIFEST {
        return Err(format!("{FILE} is larger than 64 KiB"));
    }
    parse(&text)
}

type Key = (u64, u64, u64, i64, i64, i64, i64);
type Sums = std::collections::HashMap<PathBuf, (Key, Vec<u8>)>;
/// What is known of each file the fingerprint read: where it was, and its SHA-256.
static READ: std::sync::Mutex<Option<Sums>> = std::sync::Mutex::new(None);

/// The folder `from` was renamed `to`: what is known of its files goes with it, so that big
/// downloaded files, hashed while they were staged, are not read again at the first look.
fn moved(from: &Path, to: &Path) {
    let mut read = READ.lock().unwrap_or_else(|e| e.into_inner());
    let Some(read) = read.as_mut() else { return };
    read.retain(|path, _| !path.starts_with(to));
    let known: Vec<PathBuf> = read
        .keys()
        .filter(|p| p.starts_with(from))
        .cloned()
        .collect();
    for path in known {
        if let (Some(sum), Ok(rest)) = (read.remove(&path), path.strip_prefix(from)) {
            read.insert(to.join(rest), sum);
        }
    }
}

/// SHA-256 over the folder's entries, each folder's in order of name, depth first: for each,
/// its path from `root`, its kind, and for a file whether it runs, its length and the SHA-256
/// of its bytes. A link or special file counts as itself, so one added later changes it.
/// A file's bytes are read again only when its inode, length, mtime or ctime moved: the
/// service's one thread checks a workplace at each listing and each start of its program, and
/// ctime cannot be set back by the file's owner.
pub fn fingerprint(root: &Path) -> io::Result<String> {
    fn walk(
        root: &Path,
        at: &Path,
        depth: usize,
        seen: &mut usize,
        hash: &mut digest::Context,
    ) -> io::Result<()> {
        if depth > LIMITS.depth {
            return Err(io::Error::other("folders too deep"));
        }
        let mut names = fs::read_dir(at)?
            .map(|e| e.map(|e| e.file_name()))
            .collect::<io::Result<Vec<_>>>()?;
        names.sort();
        for name in names {
            *seen += 1;
            if *seen > LIMITS.files {
                return Err(io::Error::other("too many files"));
            }
            let path = at.join(&name);
            let meta = fs::symlink_metadata(&path)?;
            hash.update(
                path.strip_prefix(root)
                    .unwrap_or(&path)
                    .as_os_str()
                    .as_bytes(),
            );
            hash.update(&[0]);
            if meta.is_dir() {
                hash.update(b"d");
                walk(root, &path, depth + 1, seen, hash)?;
            } else if meta.is_file() {
                hash.update(&[b'f', (meta.mode() & 0o111 != 0) as u8]);
                hash.update(&meta.len().to_le_bytes());
                let key = (
                    meta.dev(),
                    meta.ino(),
                    meta.len(),
                    meta.mtime(),
                    meta.mtime_nsec(),
                    meta.ctime(),
                    meta.ctime_nsec(),
                );
                let mut read = READ.lock().unwrap_or_else(|e| e.into_inner());
                let read = read.get_or_insert_default();
                match read.get(&path) {
                    Some((k, sum)) if *k == key => hash.update(sum),
                    _ => {
                        let mut file = fs::File::open(&path)?;
                        let mut sum = digest::Context::new(&digest::SHA256);
                        let mut buf = [0u8; 64 * 1024];
                        let mut left = meta.len();
                        while left > 0 {
                            let take = buf.len().min(left as usize);
                            let n = file.read(&mut buf[..take])?;
                            if n == 0 {
                                return Err(io::Error::other("a file got shorter"));
                            }
                            sum.update(&buf[..n]);
                            left -= n as u64;
                        }
                        let sum = sum.finish().as_ref().to_vec();
                        hash.update(&sum);
                        read.insert(path.clone(), (key, sum));
                    }
                }
            } else {
                hash.update(b"o");
            }
        }
        Ok(())
    }
    let mut hash = digest::Context::new(&digest::SHA256);
    walk(root, root, 0, &mut 0, &mut hash)?;
    Ok(hash
        .finish()
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

/// The workplace as it is on disk now, against what was approved.
pub fn inspect(row: &Workplace, used_by: Vec<String>) -> Listed {
    let root = folder(&row.name);
    let opened = Dir::open(&root).map_err(|e| format!("cannot open its folder: {e}"));
    let manifest = opened.as_ref().map_err(Clone::clone).and_then(read);
    let now = fingerprint(&root);
    let mut error = manifest.as_ref().err().cloned();
    if error.is_none()
        && let Err(e) = &now
    {
        error = Some(format!("cannot read its files: {e}"));
    }
    let skills = opened
        .as_ref()
        .ok()
        .and_then(|d| d.subdir(proto::skills::DIR, false).ok())
        .and_then(|d| skills::list_in(&d).ok())
        .unwrap_or_default();
    let mut roles: Vec<PathBuf> = opened
        .as_ref()
        .ok()
        .and_then(|d| d.subdir("roles", false).ok())
        .and_then(|d| d.names().ok().map(|names| (d, names)))
        .map(|(d, names)| {
            names
                .into_iter()
                .filter(|n| n.ends_with(".md") && d.regular(n).ok().flatten().is_some())
                .map(|n| root.join("roles").join(n))
                .collect()
        })
        .unwrap_or_default();
    roles.sort();
    Listed {
        name: row.name.clone(),
        version: row.version.clone(),
        manifest: manifest.ok(),
        skills,
        roles,
        changed: now.ok().as_deref() != Some(row.fingerprint.as_str()),
        error,
        used_by,
    }
}

type Refusal = (&'static str, String);

/// The manifest of the folder `source`, for the person to see before installing.
pub fn preview(source: &Path) -> Result<Manifest, Refusal> {
    let invalid = |e: String| ("invalid_workplace", e);
    let from = Dir::open(source)
        .map_err(|e| invalid(format!("{} is not a folder: {e}", source.display())))?;
    read(&from).map_err(invalid)
}

/// A workplace copied and fetched into a staging folder, ready to be moved into place.
pub struct Prepared {
    pub manifest: Manifest,
    staged: PathBuf,
    fingerprint: String,
}

/// Copies the folder `source` and fetches what its manifest lists into a staging folder: the
/// slow part of an install, for a thread of its own. Downloads need `fetch` (the person saw
/// their size and hosts and agreed). `report` gets the bytes fetched so far and the total.
/// Whatever fails leaves nothing behind.
pub fn prepare(
    source: &Path,
    fetch: bool,
    in_use: impl FnOnce(&str) -> bool,
    report: &mut dyn FnMut(&str, u64, u64),
) -> Result<Prepared, Refusal> {
    let invalid = |e: String| ("invalid_workplace", e);
    // Held open like a skill's folder: the picked folder may be one a bot can write in.
    let from = Dir::open(source)
        .map_err(|e| invalid(format!("{} is not a folder: {e}", source.display())))?;
    let manifest = read(&from).map_err(invalid)?;
    if in_use(&manifest.name) {
        return Err(in_use_refusal(&manifest.name));
    }
    if !manifest.downloads.is_empty() && !fetch {
        return Err((
            "download_not_agreed",
            format!(
                "the workplace {} downloads {} bytes from {}: say `download: true` once the person has agreed",
                manifest.name,
                manifest.download_bytes(),
                manifest.hosts().join(", ")
            ),
        ));
    }
    let failed = |e: io::Error| invalid(format!("cannot install the workplace: {e}"));
    let staged = skills::staging().map_err(failed)?;
    let scratch = skills::staging().map_err(failed)?;
    let name = manifest.name.clone();
    let prepared = skills::copy(&from, &staged, &LIMITS, 0, &mut (0, 0))
        .map_err(invalid)
        .and_then(|()| {
            // The archives are unpacked from here, apart from the workplace's folder.
            sys::create_private_dir(&scratch).map_err(failed)?;
            fetch_all(&manifest, &staged, &scratch, &mut |done, total| {
                report(&name, done, total)
            })
            .map_err(|e| ("download_failed", e))?;
            check_program(&manifest, &staged).map_err(invalid)?;
            // The downloaded files count in it like the rest: too many is found here.
            let fingerprint = fingerprint(&staged).map_err(failed)?;
            Ok(fingerprint)
        });
    skills::discard(Some(scratch));
    match prepared {
        Ok(fingerprint) => Ok(Prepared {
            manifest,
            staged,
            fingerprint,
        }),
        Err(e) => {
            skills::discard(Some(staged));
            Err(e)
        }
    }
}

fn in_use_refusal(name: &str) -> Refusal {
    (
        "workplace_in_use",
        format!("the workplace {name} is switched on in a project: switch it off first"),
    )
}

/// Moves a prepared workplace into place, in place of one of the same name unless `in_use`
/// says a project has it on. Gives what to record.
pub fn commit(prepared: Prepared, in_use: impl FnOnce(&str) -> bool) -> Result<Workplace, Refusal> {
    let Prepared {
        manifest,
        staged,
        fingerprint,
    } = prepared;
    let failed = |e: io::Error| {
        (
            "invalid_workplace",
            format!("cannot install the workplace: {e}"),
        )
    };
    let installed = if in_use(&manifest.name) {
        Err(in_use_refusal(&manifest.name))
    } else {
        (|| {
            sys::create_private_dir(&dir()).map_err(failed)?;
            let target = folder(&manifest.name);
            let old = skills::staging().map_err(failed)?;
            let moved_out = match fs::rename(&target, &old) {
                Ok(()) => true,
                Err(e) if e.kind() == io::ErrorKind::NotFound => false,
                Err(e) => return Err(failed(e)),
            };
            if let Err(e) = fs::rename(&staged, &target) {
                if moved_out {
                    let _ = fs::rename(&old, &target);
                }
                return Err(failed(e));
            }
            moved(&staged, &target);
            if moved_out {
                skills::discard(Some(old));
            }
            Ok(Workplace {
                name: manifest.name.clone(),
                version: manifest.version.clone(),
                fingerprint,
            })
        })()
    };
    if installed.is_err() {
        skills::discard(Some(staged));
    }
    installed
}

/// Fetches each download of the manifest into `staged`, one after the other. A download is
/// hashed as it arrives and stops at its stated size; it never writes where something is
/// already, and an archive is unpacked only after its hash is right.
fn fetch_all(
    manifest: &Manifest,
    staged: &Path,
    scratch: &Path,
    report: &mut dyn FnMut(u64, u64),
) -> Result<(), String> {
    let total = manifest.download_bytes();
    let mut before = 0;
    for d in &manifest.downloads {
        let dest = staged.join(&d.dest);
        if fs::symlink_metadata(&dest).is_ok() {
            return Err(format!(
                "`{}` is in the workplace's folder already: a download needs a place of its own",
                d.dest
            ));
        }
        let archive = scratch.join("archive");
        let (target, parent) = match d.unpack {
            Some(_) => (&archive, dest.as_path()),
            None => (&dest, dest.parent().unwrap_or(staged)),
        };
        fs::create_dir_all(parent)
            .map_err(|e| format!("cannot make the place for `{}`: {e}", d.dest))?;
        // The time allowed grows with the size: 100 KiB a second is slow enough.
        let agent = download::agent(0, Duration::from_secs(600 + d.size / (100 << 10)));
        let mut last = Instant::now();
        let got = download::to_file(&agent, &d.url, d.size, target, |n| {
            if last.elapsed() >= Duration::from_millis(250) {
                last = Instant::now();
                report(before + n, total);
            }
        })
        .map_err(|e| format!("`{}`: {e}", d.dest))?;
        if got.size != d.size {
            return Err(format!(
                "{} is {} bytes, not the {} the manifest says",
                d.url, got.size, d.size
            ));
        }
        if got.sha256 != d.sha256 {
            return Err(format!(
                "the sha256 of {} is {}, not the manifest's {}",
                d.url, got.sha256, d.sha256
            ));
        }
        if d.unpack.is_some() {
            unpack(&archive, &dest)?;
            fs::remove_file(&archive).map_err(|e| format!("cannot remove the archive: {e}"))?;
        }
        before += d.size;
        report(before, total);
    }
    Ok(())
}

/// Unpacks the `.tar.gz` at `archive` into the folder `dest` with `/usr/bin/tar`, once the
/// listing shows that every entry is a plain file or folder and stays inside `dest`: no
/// absolute path, no `..`, no link, device or other special file.
// shortcut: the unpacked size is not capped (the hash pins the archive, and its maker is the
// workplace's, who also supplies the program); cap it if archives from others are ever taken.
fn unpack(archive: &Path, dest: &Path) -> Result<(), String> {
    let tar = |flags: &str, into: Option<&Path>| {
        let mut tar = Command::new("/usr/bin/tar");
        tar.arg(flags).arg(archive);
        if let Some(into) = into {
            tar.arg("-C").arg(into);
        }
        let out =
            (tar.stdin(Stdio::null()).output()).map_err(|e| format!("cannot run tar: {e}"))?;
        if !out.status.success() {
            return Err(format!(
                "cannot unpack the archive: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
        String::from_utf8(out.stdout)
            .map_err(|_| "the archive has names that are not text".to_owned())
    };
    let (names, kinds) = (tar("-tzf", None)?, tar("-tvzf", None)?);
    if names.lines().count() != kinds.lines().count() || names.lines().count() > LIMITS.files {
        return Err(format!(
            "the archive has more than {} files and folders, or a listing that cannot be read",
            LIMITS.files
        ));
    }
    for (name, kind) in names.lines().zip(kinds.lines()) {
        let inside = Path::new(name)
            .components()
            .all(|c| matches!(c, Component::Normal(_) | Component::CurDir));
        if name.is_empty() || !inside {
            return Err(format!(
                "the archive has {name:?}, which is outside its folder"
            ));
        }
        // `-` a file, `d` a folder; a link shows as `l` or `h`, a device or pipe as another.
        if !kind.starts_with(['-', 'd']) {
            return Err(format!(
                "the archive has {name:?}, which is not a plain file or folder"
            ));
        }
    }
    tar("-xzf", Some(dest)).map(drop)
}

/// A program that is a file of the workplace must be there and run.
fn check_program(manifest: &Manifest, root: &Path) -> Result<(), String> {
    let command = &manifest.program.command;
    if !command.contains('/') {
        return Ok(());
    }
    match fs::metadata(root.join(command)) {
        Ok(meta) if meta.is_file() && meta.mode() & 0o100 != 0 => Ok(()),
        Ok(_) => Err(format!("`{command}` is not a file that can run")),
        Err(_) => Err(format!("`{command}` is not in the workplace's folder")),
    }
}

/// What approving records: the fingerprint of the files as they are, if the manifest is fine.
pub fn approve(name: &str) -> Result<Workplace, String> {
    let root = folder(name);
    let manifest = Dir::open(&root)
        .map_err(|e| format!("cannot open its folder: {e}"))
        .and_then(|d| read(&d))?;
    check_program(&manifest, &root)?;
    Ok(Workplace {
        name: name.to_owned(),
        version: manifest.version,
        fingerprint: fingerprint(&root).map_err(|e| format!("cannot read its files: {e}"))?,
    })
}

/// Takes the workplace's folder away, off the service's one thread.
pub fn remove(name: &str) -> io::Result<()> {
    let out = skills::staging()?;
    match fs::rename(folder(name), &out) {
        Ok(()) => {
            skills::discard(Some(out));
            Ok(())
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_manifest_is_checked() {
        let ok = r#"{"name":"counter","version":"1","program":{"command":"server.py"}}"#;
        let m = parse(ok).unwrap();
        assert_eq!(m.needs, Needs::default());
        assert!(m.program.args.is_empty());
        let bad = |from: &str, to: &str| parse(&ok.replace(from, to)).is_err();
        assert!(bad("counter", "Bad Name"));
        assert!(bad("counter", "a__b"));
        assert!(bad("\"1\"", "\"\""));
        assert!(bad("server.py", "/bin/sh"));
        assert!(bad("server.py", "../server.py"));
        assert!(bad("server.py", " "));
        assert!(parse(r#"{"name":"counter","version":"1"}"#).is_err());
        assert!(parse("[]").is_err());
        assert!(!bad("server.py", "python3"));
    }

    #[test]
    fn the_fingerprint_follows_paths_contents_and_the_run_bit() {
        let root = std::env::temp_dir().join(format!("appd-{}-fingerprint", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("a")).unwrap();
        fs::write(root.join("a/x"), "one").unwrap();
        let first = fingerprint(&root).unwrap();
        assert_eq!(first, fingerprint(&root).unwrap());
        assert_eq!(first.len(), 64);
        fs::write(root.join("a/x"), "two").unwrap();
        let second = fingerprint(&root).unwrap();
        assert_ne!(first, second);
        fs::rename(root.join("a/x"), root.join("a/y")).unwrap();
        let third = fingerprint(&root).unwrap();
        assert_ne!(second, third);
        std::os::unix::fs::symlink("y", root.join("a/z")).unwrap();
        assert_ne!(third, fingerprint(&root).unwrap());
        fs::remove_dir_all(&root).unwrap();
    }
}
