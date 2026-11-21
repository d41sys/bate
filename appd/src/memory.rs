//! Memory, for the window's panel: the Markdown files in `memory/` in a bot's folder, or in
//! `.bate/memory/` in a project's. A bot writes there too and can swap anything for a link at
//! any moment, so the folder is held open (`sys::Dir`) and only plain files directly in it are
//! listed, read or replaced.

use crate::paths;
use crate::store::Store;
use crate::sys::Dir;
use proto::memory::{DIR, PROJECT, is_note_name};
use serde::Serialize;
use std::io::{self, Read};
use std::path::PathBuf;
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

fn open(bot: &str, name: &str, create: bool) -> io::Result<Dir> {
    Dir::open(&paths::bot_dir(bot))?
        .subdir(name, create)
        .map_err(|e| match e.kind() {
            io::ErrorKind::NotFound => e,
            _ => io::Error::other(format!("`{name}` in the bot's folder is not a folder: {e}")),
        })
}

/// The folder `name` in the bot's folder, held open. `None`: there is none. An error if a link
/// or a file has that name.
pub fn folder(bot: &str, name: &str) -> io::Result<Option<Dir>> {
    match open(bot, name, false) {
        Ok(dir) => Ok(Some(dir)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// The same, made first if missing. An error too if the bot takes it away in between.
pub fn made(bot: &str, name: &str) -> io::Result<Dir> {
    open(bot, name, true)
}

/// Where notes are kept: a folder and the folders in it down to the notes'.
pub struct Place(PathBuf, &'static [&'static str]);

impl Place {
    pub fn bot(bot: &str) -> Place {
        Place(paths::bot_dir(bot), &[DIR])
    }

    /// `folder`: the project's, kept by its real path, so nothing above it is a link.
    pub fn project(folder: &str) -> Place {
        Place(folder.into(), &[PROJECT, DIR])
    }

    /// The full path of the notes' folder, which is how copies of its notes are found. The
    /// folder in the bot's or the project's is the service's own, so nothing above it is a link
    /// that matters; what lies beyond it is not looked up.
    fn folder(&self) -> PathBuf {
        let base = self.0.canonicalize().unwrap_or_else(|_| self.0.clone());
        self.1.iter().fold(base, |dir, part| dir.join(part))
    }

    /// The full path of the note `name`.
    pub fn path(&self, name: &str) -> String {
        self.folder().join(name).to_string_lossy().into_owned()
    }

    /// The notes' folder held open, made first if `create`; `None`: there is none.
    fn dir(&self, create: bool) -> io::Result<Option<Dir>> {
        let mut dir = Dir::open(&self.0)?;
        for part in self.1 {
            dir = match dir.subdir(part, create) {
                Ok(dir) => dir,
                Err(e) if e.kind() == io::ErrorKind::NotFound && !create => return Ok(None),
                Err(e) => return Err(io::Error::other(format!("`{part}` is not a folder: {e}"))),
            };
        }
        Ok(Some(dir))
    }
}

/// The notes, by name.
pub fn list(place: &Place) -> io::Result<Vec<Note>> {
    let Some(dir) = place.dir(false)? else {
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
pub fn read(place: &Place, name: &str) -> io::Result<Option<String>> {
    let Some(file) = place
        .dir(false)?
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
pub fn write(place: &Place, name: &str, bytes: &[u8]) -> io::Result<()> {
    place.dir(true)?.expect("made").put(name, bytes, None)
}

/// The note's bytes as they are, to keep before it is replaced or removed; `None`: no plain
/// file, or one over `proto::undo::MAX_FILE`.
pub fn current(place: &Place, name: &str) -> io::Result<Option<Vec<u8>>> {
    let Some(file) = place
        .dir(false)?
        .map(|d| d.read(name))
        .transpose()?
        .flatten()
    else {
        return Ok(None);
    };
    let mut bytes = Vec::new();
    file.take(proto::undo::MAX_FILE + 1)
        .read_to_end(&mut bytes)?;
    Ok((bytes.len() as u64 <= proto::undo::MAX_FILE).then_some(bytes))
}

/// A note that is gone, and its newest earlier version.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Deleted {
    pub name: String,
    pub bytes: u64,
    pub at_ms: u64,
    pub by: String,
    pub id: String,
}

/// The notes of `place` that have an earlier version and are not there now, newest first.
pub fn deleted(place: &Place, store: &Store) -> Result<Vec<Deleted>, String> {
    let dir = place.folder();
    let dir = dir.to_string_lossy();
    let mut gone = Vec::new();
    for (path, version) in store.latest_under(&dir).map_err(|e| e.to_string())? {
        let name = &path[dir.len() + 1..];
        if !is_note_name(name) {
            continue;
        }
        let there = match place.dir(false) {
            Ok(Some(dir)) => dir.read(name).map_err(|e| e.to_string())?.is_some(),
            Ok(None) => false,
            Err(e) => return Err(e.to_string()),
        };
        if !there {
            gone.push(Deleted {
                name: name.to_owned(),
                bytes: version.bytes,
                at_ms: version.at_ms,
                by: version.by,
                id: version.id,
            });
        }
    }
    Ok(gone)
}

/// Removes the note (or the link under its name); false if there was none.
pub fn delete(place: &Place, name: &str) -> io::Result<bool> {
    match place.dir(false)? {
        Some(dir) => dir.remove(name),
        None => Ok(false),
    }
}
