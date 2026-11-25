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

/// Limits of one folder copied in: a mistaken choice (a home folder) must not be copied.
pub struct Limits {
    pub files: usize,
    pub bytes: u64,
    pub depth: usize,
    /// What the folder is, for the refusals.
    pub what: &'static str,
}

const SKILL: Limits = Limits {
    files: 500,
    bytes: 20 * 1024 * 1024,
    depth: 12,
    what: "skill",
};
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
    match folder(bot, DIR)? {
        Some(dir) => list_in(&dir),
        None => Ok(Vec::new()),
    }
}

/// The skills in the folder `dir` (a `skills/`), by name.
pub fn list_in(dir: &Dir) -> io::Result<Vec<Skill>> {
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
pub fn staging() -> io::Result<PathBuf> {
    let dir = paths::home().join("staging");
    sys::create_private_dir(&dir)?;
    Ok(dir.join(sys::random_hex(8)))
}

/// The folder `source`, held open, and the skill its `SKILL.md` describes.
fn open(source: &Path) -> Result<(Dir, Skill), String> {
    // Held open like a bot's folder: the picked folder may be one a bot can write in.
    let from =
        Dir::open(source).map_err(|e| format!("{} is not a folder: {e}", source.display()))?;
    let skill = match from.regular(FILE) {
        Ok(Some(file)) => parse(file)?,
        Ok(None) => return Err(format!("the folder has no {FILE}")),
        Err(e) => return Err(format!("cannot read {FILE}: {e}")),
    };
    Ok((from, skill))
}

/// What `install` would copy in, checked as it checks: the skill, and how many files and
/// folders and how many bytes the folder holds. Nothing is kept.
pub fn preview(source: &Path) -> Result<(Skill, usize, u64), String> {
    let (from, skill) = open(source)?;
    let failed = |e: io::Error| format!("cannot look at the skill: {e}");
    let staged = staging().map_err(failed)?;
    let mut seen = (0, 0);
    let counted = copy(&from, &staged, &SKILL, 0, &mut seen);
    discard(Some(staged));
    counted.map(|()| (skill, seen.0, seen.1))
}

/// Copies the skill folder `source` into the bot's skills, in place of one of the same name.
pub fn install(bot: &str, source: &Path) -> Result<Skill, String> {
    let (from, skill) = open(source)?;
    let failed = |e: io::Error| format!("cannot install the skill: {e}");
    let staged = staging().map_err(failed)?;
    let installed = copy(&from, &staged, &SKILL, 0, &mut (0, 0)).and_then(|()| {
        let dir = made(bot, DIR).map_err(failed)?;
        let out = staging().map_err(failed)?;
        let had = swap(&dir, &staged, &skill.name, &out).map_err(failed)?;
        discard(had.then_some(out));
        Ok(())
    });
    if installed.is_err() {
        discard(Some(staged));
    }
    installed.map(|()| skill)
}

/// Moves `staged` in as `name`, in place of what is there, which goes to `out` (true if there
/// was something). If the new one cannot be moved in, the old one is put back.
fn swap(dir: &Dir, staged: &Path, name: &str, out: &Path) -> io::Result<bool> {
    let had = dir.give(name, out)?;
    if let Err(e) = dir.take(staged, name) {
        if had {
            let _ = dir.take(out, name);
        }
        return Err(e);
    }
    Ok(had)
}

/// Makes the skill `name` in the bot's skills from `files` (paths already checked to stay
/// inside the folder, each with its text and whether it is runnable): written in the staging
/// folder, moved in whole. A runnable file gets the mode `copy` gives one (owner rwx, group and
/// others r-x); never anything more.
pub fn install_texts(bot: &str, name: &str, files: &[(&str, &str, bool)]) -> Result<(), String> {
    let failed = |e: io::Error| format!("cannot install the skill {name}: {e}");
    let staged = staging().map_err(failed)?;
    let installed = (|| {
        fs::DirBuilder::new().create(&staged)?;
        for (path, text, runnable) in files {
            let to = staged.join(path);
            if let Some(parent) = to.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(if *runnable { 0o755 } else { 0o644 })
                .open(&to)?;
            io::Write::write_all(&mut file, text.as_bytes())?;
        }
        let dir = made(bot, DIR)?;
        let old = remove(&dir, name)?;
        let moved = dir.take(&staged, name);
        discard(old);
        moved
    })();
    if installed.is_err() {
        discard(Some(staged));
    }
    installed.map_err(failed)
}

/// Copies a folder of plain files and folders to the new folder `to`. `seen` counts entries
/// and bytes so far.
pub fn copy(
    from: &Dir,
    to: &Path,
    limits: &Limits,
    depth: usize,
    seen: &mut (usize, u64),
) -> Result<(), String> {
    let Limits {
        files,
        bytes,
        depth: deepest,
        what,
    } = *limits;
    let failed = |e: io::Error| format!("cannot copy the {what} folder: {e}");
    let too_big = || {
        format!(
            "the {what} folder has more than {files} files and folders or {} MiB",
            bytes / (1024 * 1024)
        )
    };
    if depth > deepest {
        return Err(format!(
            "the {what} folder is more than {deepest} folders deep"
        ));
    }
    fs::DirBuilder::new().create(to).map_err(failed)?;
    for name in from.names().map_err(failed)? {
        seen.0 += 1;
        if seen.0 > files {
            return Err(too_big());
        }
        let target = to.join(&name);
        if let Ok(folder) = from.subdir(&name, false) {
            copy(&folder, &target, limits, depth + 1, seen)?;
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
            let left = bytes - seen.1 + 1;
            seen.1 += io::copy(&mut input.take(left), &mut output).map_err(failed)?;
            if seen.1 > bytes {
                return Err(too_big());
            }
        } else {
            return Err(format!(
                "{name} is a link or a special file; a {what} has only files and folders"
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
pub fn discard(path: Option<PathBuf>) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_skill_that_cannot_be_moved_in_leaves_the_old_one_as_it_was() {
        let root = std::env::temp_dir().join(format!("appd-{}-skill-swap", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("skills/x")).unwrap();
        fs::write(root.join("skills/x/SKILL.md"), "old").unwrap();
        let dir = Dir::open(&root.join("skills")).unwrap();
        let (out, text) = (root.join("out"), root.join("skills/x/SKILL.md"));

        // Nothing staged: the move in fails.
        assert!(swap(&dir, &root.join("none"), "x", &out).is_err());
        assert_eq!(fs::read_to_string(&text).unwrap(), "old");
        assert!(!out.exists());

        fs::create_dir_all(root.join("new")).unwrap();
        fs::write(root.join("new/SKILL.md"), "new").unwrap();
        assert!(swap(&dir, &root.join("new"), "x", &out).unwrap());
        assert_eq!(fs::read_to_string(&text).unwrap(), "new");
        assert_eq!(fs::read_to_string(out.join("SKILL.md")).unwrap(), "old");
        fs::remove_dir_all(&root).unwrap();
    }
}
