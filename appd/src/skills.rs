//! A bot's skills, for the window: installing a skill folder, listing, removing. Skills live
//! in `skills/<name>/` in the bot's folder. The bot can write there and swap anything for a
//! link at any moment, so, as with memory, the folder is held open (`sys::Dir`), and a skill is
//! copied in the service's own folder and moved in or out whole.

use crate::memory::folder;
use crate::sys::Dir;
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
    let Some(dir) = folder(bot, DIR, false)? else {
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
    if !fs::symlink_metadata(source).is_ok_and(|meta| meta.is_dir()) {
        return Err(format!("{} is not a folder", source.display()));
    }
    let skill = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(source.join(FILE))
        .map_err(|e| format!("cannot read {FILE}: {e}"))
        .and_then(parse)?;
    let failed = |e: io::Error| format!("cannot install the skill: {e}");
    let staged = staging().map_err(failed)?;
    let installed = copy(source, &staged, 0, &mut (0, 0)).and_then(|()| {
        let dir = folder(bot, DIR, true)
            .map_err(failed)?
            .expect("made if missing");
        remove(&dir, &skill.name).map_err(failed)?;
        dir.take(&staged, &skill.name).map_err(failed)
    });
    if installed.is_err() {
        let _ = fs::remove_dir_all(&staged);
    }
    installed.map(|()| skill)
}

/// Copies a folder of plain files and folders. `seen` counts files and bytes so far.
fn copy(from: &Path, to: &Path, depth: usize, seen: &mut (usize, u64)) -> Result<(), String> {
    let failed = |e: io::Error| format!("cannot copy {}: {e}", from.display());
    if depth > MAX_DEPTH {
        return Err(format!(
            "the skill folder is more than {MAX_DEPTH} folders deep"
        ));
    }
    fs::DirBuilder::new().create(to).map_err(failed)?;
    for entry in fs::read_dir(from).map_err(failed)? {
        let entry = entry.map_err(failed)?;
        let (source, target) = (entry.path(), to.join(entry.file_name()));
        // `DirEntry::metadata` follows no link.
        let meta = entry.metadata().map_err(failed)?;
        if meta.is_dir() {
            copy(&source, &target, depth + 1, seen)?;
        } else if meta.is_file() {
            *seen = (seen.0 + 1, seen.1 + meta.len());
            if seen.0 > MAX_FILES || seen.1 > MAX_BYTES {
                return Err(format!(
                    "the skill folder has more than {MAX_FILES} files or {} MiB",
                    MAX_BYTES / (1024 * 1024)
                ));
            }
            let mut input = fs::File::open(&source).map_err(failed)?;
            let mut output = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                // Scripts stay runnable; nothing becomes writable by others.
                .mode(meta.permissions().mode() & 0o755)
                .open(&target)
                .map_err(failed)?;
            io::copy(&mut input, &mut output).map_err(failed)?;
        } else {
            return Err(format!(
                "{} is a link or a special file; a skill has only files and folders",
                source.display()
            ));
        }
    }
    Ok(())
}

/// Removes `name` from the skills folder, whatever it is: moved out whole to the service's
/// own folder first, so nothing is deleted through a link. False if there was none.
fn remove(dir: &Dir, name: &str) -> io::Result<bool> {
    let out = staging()?;
    if !dir.give(name, &out)? {
        return Ok(false);
    }
    // On a link this removes the link itself.
    match fs::symlink_metadata(&out)?.is_dir() {
        true => fs::remove_dir_all(&out)?,
        false => fs::remove_file(&out)?,
    }
    Ok(true)
}

/// Removes the skill; false if there was none.
pub fn delete(bot: &str, name: &str) -> io::Result<bool> {
    match folder(bot, DIR, false)? {
        Some(dir) => remove(&dir, name),
        None => Ok(false),
    }
}
