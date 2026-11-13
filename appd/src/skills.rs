//! A bot's skills, for the window: installing a skill folder, listing, removing. Skills live
//! in `skills/<name>/` in the bot's folder. The bot can write there and swap anything for a
//! link at any moment, so, as with memory, the folder is held open (`sys::Dir`), and a skill is
//! copied in the service's own folder and moved in or out whole.

use crate::memory::{folder, made};
use crate::sys::Dir;
use crate::sys::log;
use crate::{paths, sys};
use proto::skills::{self, DIR, FILE, Skill};
use std::fs;
use std::io::{self, Read};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

/// Limits of one skill folder: a mistaken choice (a home folder) must not be copied.
const MAX_FILES: usize = 500;
const MAX_BYTES: u64 = 20 * 1024 * 1024;
const MAX_DEPTH: usize = 12;
/// Most bytes read of a `SKILL.md`.
const MAX_FILE: u64 = 64 * 1024;

fn parse(file: fs::File) -> Result<Skill, String> {
    let mut text = String::new();
    file.take(MAX_FILE)
        .read_to_string(&mut text)
        .map_err(|e| format!("cannot read {FILE}: {e}"))?;
    skills::parse(&text)
}

/// The installed skills, by name. A folder that is not a skill is left out.
pub fn list(bot: &str) -> io::Result<Vec<Skill>> {
    let Some(dir) = folder(bot, DIR)? else {
        return Ok(Vec::new());
    };
    let mut found = Vec::new();
    for name in dir.names()? {
        // A link, a file, a folder without a plain `SKILL.md`: not a skill.
        let Ok(Some(file)) = dir.subdir(&name, false).and_then(|skill| skill.read(FILE)) else {
            continue;
        };
        if let Ok(skill) = parse(file)
            && skill.name == name
        {
            found.push(skill);
        }
    }
    found.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(found)
}

/// A new path in the service's own staging folder, which no bot can reach.
fn staging() -> io::Result<PathBuf> {
    let dir = paths::home().join("staging");
    sys::create_private_dir(&dir)?;
    Ok(dir.join(sys::random_hex(8)))
}

/// Copies the skill folder `source` into the bot's skills, in place of one of the same name.
pub fn install(bot: &str, source: &Path) -> Result<Skill, String> {
    // Held open like a bot's folder: the picked folder may be one a bot can write in.
    let from =
        Dir::open(source).map_err(|e| format!("{} is not a folder: {e}", source.display()))?;
    let skill = match from.regular(FILE) {
        Ok(Some(file)) => parse(file)?,
        Ok(None) => return Err(format!("the folder has no {FILE}")),
        Err(e) => return Err(format!("cannot read {FILE}: {e}")),
    };
    let failed = |e: io::Error| format!("cannot install the skill: {e}");
    let staged = staging().map_err(failed)?;
    let installed = copy(&from, &staged, 0, &mut (0, 0)).and_then(|()| {
        let dir = made(bot, DIR).map_err(failed)?;
        let old = remove(&dir, &skill.name).map_err(failed)?;
        let moved = dir.take(&staged, &skill.name).map_err(failed);
        discard(old);
        moved
    });
    if installed.is_err() {
        discard(Some(staged));
    }
    installed.map(|()| skill)
}

/// Copies a folder of plain files and folders to the new folder `to`. `seen` counts entries
/// and bytes so far.
fn copy(from: &Dir, to: &Path, depth: usize, seen: &mut (usize, u64)) -> Result<(), String> {
    let failed = |e: io::Error| format!("cannot copy the skill folder: {e}");
    let too_big = || {
        format!(
            "the skill folder has more than {MAX_FILES} files and folders or {} MiB",
            MAX_BYTES / (1024 * 1024)
        )
    };
    if depth > MAX_DEPTH {
        return Err(format!(
            "the skill folder is more than {MAX_DEPTH} folders deep"
        ));
    }
    fs::DirBuilder::new().create(to).map_err(failed)?;
    for name in from.names().map_err(failed)? {
        seen.0 += 1;
        if seen.0 > MAX_FILES {
            return Err(too_big());
        }
        let target = to.join(&name);
        if let Ok(folder) = from.subdir(&name, false) {
            copy(&folder, &target, depth + 1, seen)?;
        } else if let Some(input) = from.regular(&name).map_err(failed)? {
            let mode = input.metadata().map_err(failed)?.permissions().mode();
            let mut output = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                // Scripts stay runnable; nothing becomes writable by others, or setuid.
                .mode(mode & 0o755)
                .open(&target)
                .map_err(failed)?;
            // One byte past what is left: the file may grow while it is copied.
            let left = MAX_BYTES - seen.1 + 1;
            seen.1 += io::copy(&mut input.take(left), &mut output).map_err(failed)?;
            if seen.1 > MAX_BYTES {
                return Err(too_big());
            }
        } else {
            return Err(format!(
                "{name} is a link or a special file; a skill has only files and folders"
            ));
        }
    }
    Ok(())
}

/// Moves `name` out of the skills folder, whatever it is, to the service's own folder, so
/// nothing is deleted through a link. Gives where it is now, for `discard`.
fn remove(dir: &Dir, name: &str) -> io::Result<Option<PathBuf>> {
    let out = staging()?;
    Ok(dir.give(name, &out)?.then_some(out))
}

/// Deletes what was moved out or staged, off the service's one thread: the bot decides how
/// much there is. What is left goes when the service next starts (`sweep`).
fn discard(path: Option<PathBuf>) {
    let Some(path) = path else { return };
    std::thread::spawn(move || {
        if !fs::symlink_metadata(&path).is_ok_and(|meta| meta.is_dir()) {
            // A link too: the link itself goes.
            let _ = fs::remove_file(&path);
        } else if fs::remove_dir_all(&path).is_err() {
            // A bot can leave folders nobody may enter; the owner may still open them up.
            let _ = std::process::Command::new("/bin/chmod")
                .args(["-R", "u+rwx"])
                .arg(&path)
                .status();
            if let Err(e) = fs::remove_dir_all(&path) {
                log!("{} not removed: {e}", path.display());
            }
        }
    });
}

/// Empties the staging folder of what a crash left there.
pub fn sweep() {
    let dir = paths::home().join("staging");
    for entry in fs::read_dir(&dir).into_iter().flatten().flatten() {
        discard(Some(entry.path()));
    }
}

/// Removes the skill; false if there was none.
pub fn delete(bot: &str, name: &str) -> io::Result<bool> {
    let Some(dir) = folder(bot, DIR)? else {
        return Ok(false);
    };
    let old = remove(&dir, name)?;
    let found = old.is_some();
    discard(old);
    Ok(found)
}
