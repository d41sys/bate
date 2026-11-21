//! Undo of a bot's file change: puts the earlier content back, or removes a file the change
//! made. The service does it, never the bot's engine, and a bot can swap anything in its own
//! folder for a link at any moment, so every folder on the way is opened without following a
//! link (`sys::Dir`) and the file is replaced by a new one, not written through.

use crate::store::{Kept, Owner, Store};
use crate::sys::{Dir, now_ms, random_hex};
use proto::undo::MAX_FILE;
use ring::digest::{Context, SHA256};
use std::io::{self, Read};
use std::os::unix::fs::PermissionsExt;
use std::path::{Component, Path, PathBuf};

/// Why a change was not undone.
pub enum Failure {
    /// The file is no longer as the change left it.
    Changed,
    Other(String),
}

impl From<io::Error> for Failure {
    fn from(e: io::Error) -> Failure {
        Failure::Other(e.to_string())
    }
}

/// The folder that holds `path` (held open) and the file's name in it. `path` must be full and
/// lie in one of `roots` (the folders the bot may reach, with no link in them), or anywhere
/// with `whole`. The folders below the root are made when `create`.
fn parent(path: &str, roots: &[PathBuf], whole: bool, create: bool) -> io::Result<(Dir, String)> {
    let path = Path::new(path);
    let plain = path.is_absolute()
        && path
            .components()
            .skip(1)
            .all(|c| matches!(c, Component::Normal(_)));
    if !plain {
        return Err(io::Error::other("the file's path is not a full path"));
    }
    let root = roots
        .iter()
        .filter(|r| path.starts_with(r))
        .max_by_key(|r| r.as_os_str().len())
        .map(PathBuf::as_path)
        .or(whole.then_some(Path::new("/")))
        .ok_or_else(|| io::Error::other("the file is outside the folders this bot may change"))?;
    let rest = path.strip_prefix(root).expect("checked above");
    let mut names = Vec::new();
    for part in rest.components() {
        names.push(
            part.as_os_str()
                .to_str()
                .ok_or_else(|| io::Error::other("a name that is not text"))?,
        );
    }
    let (name, folders) = names
        .split_last()
        .ok_or_else(|| io::Error::other("the file's path is a folder"))?;
    let mut dir = Dir::open(root)?;
    for folder in folders {
        dir = dir.subdir(folder, create)?;
    }
    Ok((dir, (*name).to_owned()))
}

/// A file as it is now.
struct Now {
    /// Its hash; none when there is no plain file by that name (a link is none).
    sum: Option<String>,
    /// Its bytes, when it is at most `MAX_FILE`.
    bytes: Option<Vec<u8>>,
    mode: Option<u32>,
}

fn look(dir: &Dir, name: &str) -> io::Result<Now> {
    let Some(mut file) = dir.regular(name)? else {
        return Ok(Now {
            sum: None,
            bytes: None,
            mode: None,
        });
    };
    let meta = file.metadata()?;
    let (mut hash, mut bytes, mut chunk) = (Context::new(&SHA256), Vec::new(), vec![0; 64 << 10]);
    loop {
        let n = file.read(&mut chunk)?;
        if n == 0 {
            break;
        }
        hash.update(&chunk[..n]);
        // The file may grow while it is read: what is kept is only ever up to the bound.
        if bytes.len() as u64 <= MAX_FILE {
            bytes.extend_from_slice(&chunk[..n]);
        }
    }
    Ok(Now {
        sum: Some(proto::undo::hex(hash.finish().as_ref())),
        bytes: (bytes.len() as u64 <= MAX_FILE).then_some(bytes),
        mode: Some(meta.permissions().mode() & 0o7777),
    })
}

/// Puts the file `at` as `earlier` has it (`None`: removes it), keeping what it replaces as a
/// copy of the user's own change, so that this can be undone in turn.
fn replace(
    store: &Store,
    owner: Owner,
    kept: &Kept,
    place: (&Dir, &str),
    now: &Now,
    earlier: Option<&[u8]>,
) -> Result<(), Failure> {
    let (dir, name) = place;
    if let Some(bytes) = &now.bytes {
        let copy = Kept {
            id: random_hex(8),
            conversation: kept.conversation.clone(),
            path: kept.path.clone(),
            earlier: Some(bytes.clone()),
            after: None,
            who: "user",
            at: now_ms(),
            bot: None,
            undone: false,
        };
        store
            .keep(owner, &copy)
            .map_err(|e| Failure::Other(e.to_string()))?;
    }
    match earlier {
        Some(bytes) => dir.put(name, bytes, now.mode.or(Some(0o644)))?,
        None => {
            dir.remove(name)?;
        }
    }
    Ok(())
}

/// Undoes the change `kept` of `bot`. Without `force`, only while the file is as the change
/// left it.
pub fn undo(
    store: &Store,
    bot: &str,
    kept: &Kept,
    roots: &[PathBuf],
    whole: bool,
    force: bool,
) -> Result<(), Failure> {
    let earlier = kept.earlier.as_deref();
    let (dir, name) = match parent(&kept.path, roots, whole, earlier.is_some()) {
        Ok(found) => found,
        // A folder that is gone has no file in it.
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return if force { Ok(()) } else { Err(Failure::Changed) };
        }
        Err(e) => return Err(e.into()),
    };
    let now = look(&dir, &name)?;
    if !force && now.sum != kept.after {
        return Err(Failure::Changed);
    }
    // Forced, over a file that is not there: nothing to remove.
    if earlier.is_none() && now.sum.is_none() {
        return Ok(());
    }
    replace(store, Owner::Bot(bot), kept, (&dir, &name), &now, earlier)
}
