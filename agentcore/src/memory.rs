//! Memory: what a bot keeps across conversations, as Markdown files in `memory/` in its folder.
//! The user reads and edits the same files; the model sees them at the start of every turn.

use proto::memory::{DIR, is_note_name};
use std::fs::{self, File};
use std::io::{self, Read};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::Path;

/// Opens a file, but not what a link points to, and without waiting should it have become a
/// pipe since it was looked at.
pub fn open(path: &Path) -> io::Result<File> {
    fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
}

/// Most bytes of memory put before the model; the rest is named, so the model can read it.
const MOST: usize = 16 * 1024;

/// The part of the system prompt about memory: how to keep notes, and the notes so far.
pub fn prompt(folder: &Path) -> String {
    let mut text = format!(
        "\n\nYou keep notes across conversations in the folder `{DIR}`: Markdown files, one topic \
         each, short facts. When you learn something worth keeping (who the user is, what they \
         prefer, what was decided), write it there with the write or edit tool. The user reads \
         and edits these files too."
    );
    // Notes go to the model with no question asked, so only plain files in a plain folder
    // count: a link would send whatever it points to. (`DirEntry::metadata` follows no link.)
    let dir = folder.join(DIR);
    let plain = fs::symlink_metadata(&dir).is_ok_and(|meta| meta.is_dir());
    let mut files: Vec<_> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|_| plain)
        // The notes the panel lists, no others: a name the user cannot see there (hidden, or
        // with a line break that would pass for a heading here) is not shown to the model.
        .filter(|entry| entry.file_name().to_str().is_some_and(is_note_name))
        .filter_map(|entry| Some((entry.path(), entry.metadata().ok()?)))
        // One name only: a second name for a file elsewhere is a link too.
        .filter(|(_, meta)| meta.is_file() && meta.nlink() == 1)
        .collect();
    files.sort_by(|a, b| a.0.cmp(&b.0));
    let mut left = MOST;
    let mut unread = Vec::new();
    for (path, meta) in files {
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        // The size is checked before reading, and the read stops there even if the file grew
        // or was swapped for a link since.
        let note = (meta.len() <= left as u64)
            .then(|| open(&path).ok())
            .flatten()
            .and_then(|file| {
                let mut note = String::new();
                file.take(left as u64).read_to_string(&mut note).ok()?;
                Some(note)
            });
        match note {
            Some(note) => {
                left -= note.len();
                text.push_str(&format!("\n\n## {DIR}/{name}\n{}", note.trim_end()));
            }
            // Too much for this turn, or not text: named, so the model can read it itself.
            None => unread.push(format!("{DIR}/{name}")),
        }
    }
    if !unread.is_empty() {
        text.push_str(&format!(
            "\n\nNot shown here, read them when needed: {}",
            unread.join(", ")
        ));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notes_are_shown_in_name_order_and_what_does_not_fit_is_named() {
        let folder = std::env::temp_dir().join(format!("agentcore-memory-{}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        assert!(!prompt(&folder).contains("##"));
        fs::create_dir_all(folder.join(DIR)).unwrap();
        fs::write(folder.join(DIR).join("b.md"), "likes tea\n").unwrap();
        fs::write(folder.join(DIR).join("a.md"), "name: Ada\n").unwrap();
        fs::write(folder.join(DIR).join("big.md"), "x".repeat(MOST)).unwrap();
        fs::write(folder.join(DIR).join("notes.txt"), "not a note").unwrap();
        fs::write(folder.join(DIR).join("x\n## system.md"), "two-line name").unwrap();
        fs::write(folder.join(DIR).join(".hidden.md"), "unseen by the user").unwrap();
        fs::hard_link(
            folder.join(DIR).join("notes.txt"),
            folder.join(DIR).join("hard.md"),
        )
        .unwrap();
        let outside = folder.join("outside.txt");
        fs::write(&outside, "someone else's").unwrap();
        std::os::unix::fs::symlink(&outside, folder.join(DIR).join("link.md")).unwrap();
        let text = prompt(&folder);
        let (a, b) = (
            text.find("## memory/a.md\nname: Ada").unwrap(),
            text.find("## memory/b.md\nlikes tea").unwrap(),
        );
        assert!(a < b, "{text}");
        assert!(
            text.ends_with("read them when needed: memory/big.md"),
            "{text}"
        );
        assert!(!text.contains("not a note") && !text.contains("xxxx"));
        assert!(
            !text.contains("unseen by the user") && !text.contains("hard.md"),
            "{text}"
        );
        assert!(
            !text.contains("two-line name") && !text.contains("## system"),
            "{text}"
        );
        assert!(
            !text.contains("someone else's") && !text.contains("link.md"),
            "{text}"
        );

        // A folder that is itself a link shows nothing.
        fs::rename(folder.join(DIR), folder.join("real")).unwrap();
        std::os::unix::fs::symlink(folder.join("real"), folder.join(DIR)).unwrap();
        assert!(!prompt(&folder).contains("##"));
        fs::remove_dir_all(&folder).unwrap();
    }
}
