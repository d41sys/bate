//! A conversation: one JSON object per line in `.agentcore/sessions/<id>.jsonl` under the bot's folder.
//! A line is a chat message (it has `role`) or a marker (it has `mark`):
//! - `{"mark":"prompt","nonce"}` comes before the user message of that prompt; `rewind` cuts there.
//! - `{"mark":"compaction","summary","kept"}`: from here the model sees the summary, then the
//!   `kept` messages before the marker, then what follows.
//!
//! The file keeps everything; `messages` is what the model sees.

use serde_json::{Value, json};
use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

pub const INTERRUPTED: &str = "Interrupted: the engine stopped before this tool finished.";
const SUMMARY: &str = "Summary of the conversation so far:\n";

pub struct Session {
    pub id: String,
    path: PathBuf,
    file: File,
    /// Messages in the OpenAI chat shape (`role`, `content`, `tool_calls`, `tool_call_id`).
    pub messages: Vec<Value>,
    /// Serialised size of `messages`.
    bytes: usize,
}

fn new_id() -> io::Result<String> {
    let mut bytes = [0u8; 8];
    File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// What the model sees after reading the log `text`.
fn replay(text: &str) -> Vec<Value> {
    let mut messages = Vec::new();
    // A kill mid-write can leave a partial last line: it does not parse and is skipped.
    for line in text
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
    {
        match line["mark"].as_str() {
            Some("compaction") => {
                let kept = line["kept"].as_u64().unwrap_or(0) as usize;
                summarise(&mut messages, line["summary"].as_str().unwrap_or(""), kept);
            }
            Some(_) => {}
            None => messages.push(line),
        }
    }
    messages
}

/// Replaces all but the last `kept` messages with `summary`.
// shortcut: the summary is its own user message, so two user messages can follow each other;
// merge it into the next one if a model's chat template insists on alternating roles.
fn summarise(messages: &mut Vec<Value>, summary: &str, kept: usize) {
    let tail = messages.split_off(messages.len().saturating_sub(kept));
    messages.clear();
    messages.push(json!({"role": "user", "content": format!("{SUMMARY}{summary}")}));
    messages.extend(tail);
}

fn size(messages: &[Value]) -> usize {
    messages.iter().map(|m| m.to_string().len()).sum()
}

impl Session {
    pub fn create(dir: &Path) -> io::Result<Session> {
        let id = new_id()?;
        let path = dir.join(".agentcore/sessions").join(format!("{id}.jsonl"));
        fs::create_dir_all(path.parent().expect("has a parent"))?;
        let file = OpenOptions::new()
            .create_new(true)
            .append(true)
            .open(&path)?;
        Ok(Session {
            id,
            path,
            file,
            messages: Vec::new(),
            bytes: 0,
        })
    }

    pub fn open(dir: &Path, id: &str) -> io::Result<Session> {
        // The id comes from the service; keep it a file name.
        if id.is_empty() || id.len() > 64 || !id.bytes().all(|b| b.is_ascii_alphanumeric()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "not a session id",
            ));
        }
        let path = dir.join(".agentcore/sessions").join(format!("{id}.jsonl"));
        let log = fs::read(&path)?;
        let messages = replay(&String::from_utf8_lossy(&log));
        let mut file = OpenOptions::new().append(true).open(&path)?;
        // A line cut by a kill, even inside a character, is ended so the next line starts clean.
        if log.last().is_some_and(|&b| b != b'\n') {
            file.write_all(b"\n")?;
        }
        let bytes = size(&messages);
        Ok(Session {
            id: id.to_owned(),
            path,
            file,
            messages,
            bytes,
        })
    }

    fn write(&mut self, line: &Value) -> io::Result<usize> {
        let mut bytes = serde_json::to_vec(line)?;
        bytes.push(b'\n');
        self.file.write_all(&bytes)?;
        Ok(bytes.len())
    }

    /// Appends one message, on disk first.
    pub fn push(&mut self, message: Value) -> io::Result<()> {
        self.bytes += self.write(&message)?;
        self.messages.push(message);
        Ok(())
    }

    /// Marks where the prompt `nonce` begins, so `rewind` can return here.
    pub fn mark_prompt(&mut self, nonce: &str) -> io::Result<()> {
        self.write(&json!({"mark": "prompt", "nonce": nonce}))
            .map(drop)
    }

    /// Replaces all but the last `kept` messages with `summary`, for the model only.
    pub fn compact(&mut self, summary: &str, kept: usize) -> io::Result<()> {
        self.write(&json!({"mark": "compaction", "summary": summary, "kept": kept}))?;
        summarise(&mut self.messages, summary, kept);
        self.bytes = size(&self.messages);
        Ok(())
    }

    /// Rough size of what the model sees.
    // shortcut: 4 bytes per token; use the server's own token counts once a provider reports them (#8).
    pub fn tokens(&self) -> usize {
        self.bytes / 4
    }

    /// Goes back to just before the prompt `nonce`: in this file, or in a new one when `fork`
    /// (`id` changes and the old file is left as it was).
    pub fn rewind(&mut self, nonce: &str, fork: bool) -> io::Result<()> {
        let log = fs::read(&self.path)?;
        let text = String::from_utf8_lossy(&log);
        let mut end = 0;
        let cut = text
            .split_inclusive('\n')
            .find_map(|line| {
                let start = end;
                end += line.len();
                let line: Value = serde_json::from_str(line).ok()?;
                (line["mark"] == "prompt" && line["nonce"] == nonce).then_some(start)
            })
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("this conversation has no prompt {nonce}"),
                )
            })?;
        let kept = &text[..cut];
        if fork {
            let id = new_id()?;
            let path = self.path.with_file_name(format!("{id}.jsonl"));
            let mut file = OpenOptions::new()
                .create_new(true)
                .append(true)
                .open(&path)?;
            file.write_all(kept.as_bytes())?;
            (self.id, self.path, self.file) = (id, path, file);
        } else {
            // Written beside the file and renamed over it, so a kill leaves the old or the new one.
            let tmp = self.path.with_extension("tmp");
            fs::write(&tmp, kept)?;
            fs::rename(&tmp, &self.path)?;
            self.file = OpenOptions::new().append(true).open(&self.path)?;
        }
        self.messages = replay(kept);
        self.bytes = size(&self.messages);
        Ok(())
    }

    /// Closes every tool call that has no result (the engine was killed mid-call). Returns their ids.
    pub fn close_interrupted(&mut self) -> io::Result<Vec<String>> {
        let answered: HashSet<&str> = self
            .messages
            .iter()
            .filter_map(|m| m["tool_call_id"].as_str())
            .collect();
        let open: Vec<String> = self
            .messages
            .iter()
            .filter_map(|m| m["tool_calls"].as_array())
            .flatten()
            .filter_map(|c| c["id"].as_str())
            .filter(|id| !answered.contains(id))
            .map(str::to_owned)
            .collect();
        for id in &open {
            self.push(json!({"role": "tool", "tool_call_id": id, "content": INTERRUPTED}))?;
        }
        Ok(open)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_cut_by_a_kill_is_skipped_and_the_next_message_starts_a_new_line() {
        let dir = std::env::temp_dir().join(format!("agentcore-session-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let mut s = Session::create(&dir).unwrap();
        s.push(json!({"role": "user", "content": "first"})).unwrap();
        // Killed mid-write, inside a two-byte character.
        let cut = "{\"role\":\"assistant\",\"content\":\"é".as_bytes();
        s.file.write_all(&cut[..cut.len() - 1]).unwrap();
        let id = s.id.clone();
        drop(s);

        let mut s = Session::open(&dir, &id).unwrap();
        assert_eq!(s.messages, [json!({"role": "user", "content": "first"})]);
        s.push(json!({"role": "user", "content": "second"}))
            .unwrap();
        drop(s);
        assert_eq!(Session::open(&dir, &id).unwrap().messages.len(), 2);
        fs::remove_dir_all(&dir).unwrap();
    }
}
