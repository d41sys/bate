//! A bot's memory, for the window's panel: the Markdown files in `memory/` in the bot's folder.
//! The bot writes there too, so nothing in the folder is trusted: only plain files directly in
//! a plain `memory` folder are listed, read or replaced, never what a link points to.

use crate::{paths, sys};
use serde::Serialize;
use std::io::{Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;
use std::time::UNIX_EPOCH;
use std::{fs, io};

/// Most bytes of a note the panel is given: the bot may have written more.
const MAX_READ: u64 = 1024 * 1024;
/// Most bytes in one note written through the panel.
pub const MAX_NOTE: usize = 64 * 1024;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    pub name: String,
    pub bytes: u64,
    pub modified_ms: u64,
}

/// A file name ending in `.md`, with nothing that could leave the folder or hide the file.
pub fn check_name(name: &str) -> Result<(), String> {
    let ok = name.len() <= 100
        && name.len() > ".md".len()
        && name.ends_with(".md")
        && !name.starts_with('.')
        && !name.chars().any(|c| c == '/' || c.is_control());
    if ok {
        Ok(())
    } else {
        Err(
            "`name` must be a file name ending in .md, at most 100 bytes, not starting with a dot"
                .into(),
        )
    }
}

/// The memory folder, if it is a real folder. `None`: there is none yet.
fn dir(bot: &str) -> io::Result<Option<PathBuf>> {
    let dir = paths::bot_dir(bot).join("memory");
    match fs::symlink_metadata(&dir) {
        Ok(meta) if meta.is_dir() => Ok(Some(dir)),
        Ok(_) => Err(io::Error::other(
            "`memory` in the bot's folder is not a folder",
        )),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// The notes, by name.
pub fn list(bot: &str) -> io::Result<Vec<Note>> {
    let Some(dir) = dir(bot)? else {
        return Ok(Vec::new());
    };
    let mut notes = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        // `DirEntry::metadata` does not follow links.
        let meta = entry.metadata()?;
        if meta.is_file() && check_name(&name).is_ok() {
            let modified = meta.modified()?.duration_since(UNIX_EPOCH);
            notes.push(Note {
                name,
                bytes: meta.len(),
                modified_ms: modified.map_or(0, |d| d.as_millis() as u64),
            });
        }
    }
    notes.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(notes)
}

/// The note's text; `None` if there is no such note.
pub fn read(bot: &str, name: &str) -> io::Result<Option<String>> {
    let Some(dir) = dir(bot)? else {
        return Ok(None);
    };
    let path = dir.join(name);
    match fs::symlink_metadata(&path) {
        Ok(meta) if meta.is_file() => {
            // Not through a link put there since the check, and no more than the panel shows.
            let file = fs::OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NOFOLLOW)
                .open(path)?;
            let mut bytes = Vec::new();
            file.take(MAX_READ).read_to_end(&mut bytes)?;
            Ok(Some(String::from_utf8_lossy(&bytes).into()))
        }
        Ok(_) => Ok(None),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// Creates or replaces the note. The new file takes the name's place, so a link the bot left
/// under that name is replaced, not followed.
pub fn write(bot: &str, name: &str, text: &str) -> io::Result<()> {
    let dir = match dir(bot)? {
        Some(dir) => dir,
        None => {
            let dir = paths::bot_dir(bot).join("memory");
            sys::create_private_dir(&dir)?;
            dir
        }
    };
    let tmp = dir.join(format!(".{}.{}", sys::random_hex(4), name));
    // `create_new` does not follow a link either.
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&tmp)?;
    file.write_all(text.as_bytes())
        .and_then(|()| fs::rename(&tmp, dir.join(name)))
        .inspect_err(|_| {
            let _ = fs::remove_file(&tmp);
        })
}

/// Removes the note (or the link under its name); false if there was none.
pub fn delete(bot: &str, name: &str) -> io::Result<bool> {
    let Some(dir) = dir(bot)? else {
        return Ok(false);
    };
    match fs::remove_file(dir.join(name)) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    }
}
