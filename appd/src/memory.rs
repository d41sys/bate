//! A bot's memory, for the window's panel: the Markdown files in `memory/` in the bot's folder.
//! The bot writes there too and can swap anything for a link at any moment, so the folder is
//! held open (`sys::Dir`) and only plain files directly in it are listed, read or replaced.

use crate::paths;
use crate::sys::Dir;
use proto::memory::{DIR, is_note_name};
use serde::Serialize;
use std::io::{self, Read, Write};
use std::time::UNIX_EPOCH;

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

/// A note's name: nothing that could leave the folder or hide the file.
pub fn check_name(name: &str) -> Result<(), String> {
    if is_note_name(name) {
        Ok(())
    } else {
        Err(
            "`name` must be a file name ending in .md, at most 100 bytes, not starting with a dot"
                .into(),
        )
    }
}

/// The folder `name` in the bot's folder, held open; made first if `create`. `None`: there is
/// none. An error if a link or a file has that name.
pub fn folder(bot: &str, name: &str, create: bool) -> io::Result<Option<Dir>> {
    match Dir::open(&paths::bot_dir(bot))?.subdir(name, create) {
        Ok(dir) => Ok(Some(dir)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(io::Error::other(format!(
            "`{name}` in the bot's folder is not a folder: {e}"
        ))),
    }
}

/// The notes, by name.
pub fn list(bot: &str) -> io::Result<Vec<Note>> {
    let Some(dir) = folder(bot, DIR, false)? else {
        return Ok(Vec::new());
    };
    let mut notes = Vec::new();
    for name in dir.names()? {
        if !is_note_name(&name) {
            continue;
        }
        // A note the bot removes meanwhile, or anything that is not a plain file, is skipped.
        let Ok(Some(file)) = dir.read(&name) else {
            continue;
        };
        let meta = file.metadata()?;
        let modified = meta.modified()?.duration_since(UNIX_EPOCH);
        notes.push(Note {
            name,
            bytes: meta.len(),
            modified_ms: modified.map_or(0, |d| d.as_millis() as u64),
        });
    }
    notes.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(notes)
}

/// The note's text; `None` if there is no such note.
pub fn read(bot: &str, name: &str) -> io::Result<Option<String>> {
    let Some(file) = folder(bot, DIR, false)?
        .map(|dir| dir.read(name))
        .transpose()?
        .flatten()
    else {
        return Ok(None);
    };
    let mut bytes = Vec::new();
    file.take(MAX_READ).read_to_end(&mut bytes)?;
    Ok(Some(String::from_utf8_lossy(&bytes).into()))
}

/// Creates or replaces the note. The new file takes the name's place, so a link the bot left
/// under that name is replaced, not followed.
pub fn write(bot: &str, name: &str, text: &str) -> io::Result<()> {
    let dir = folder(bot, DIR, true)?.expect("made if missing");
    let tmp = format!(".{}.{name}", crate::sys::random_hex(4));
    let written = dir
        .create(&tmp)
        .and_then(|mut file| file.write_all(text.as_bytes()))
        .and_then(|()| dir.rename(&tmp, name));
    if written.is_err() {
        let _ = dir.remove(&tmp);
    }
    written
}

/// Removes the note (or the link under its name); false if there was none.
pub fn delete(bot: &str, name: &str) -> io::Result<bool> {
    match folder(bot, DIR, false)? {
        Some(dir) => dir.remove(name),
        None => Ok(false),
    }
}
