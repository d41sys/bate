//! A conversation: one chat message per line in `.agentcore/sessions/<id>.jsonl` under the bot's folder.

use serde_json::{Value, json};
use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

pub const INTERRUPTED: &str = "Interrupted: the engine stopped before this tool finished.";

pub struct Session {
    pub id: String,
    file: File,
    /// Messages in the OpenAI chat shape (`role`, `content`, `tool_calls`, `tool_call_id`).
    pub messages: Vec<Value>,
}

fn path(dir: &Path, id: &str) -> PathBuf {
    dir.join(".agentcore/sessions").join(format!("{id}.jsonl"))
}

impl Session {
    pub fn create(dir: &Path) -> io::Result<Session> {
        let mut bytes = [0u8; 8];
        File::open("/dev/urandom")?.read_exact(&mut bytes)?;
        let id: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        let p = path(dir, &id);
        fs::create_dir_all(p.parent().expect("has a parent"))?;
        let file = OpenOptions::new().create_new(true).append(true).open(p)?;
        Ok(Session {
            id,
            file,
            messages: Vec::new(),
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
        let p = path(dir, id);
        let text = fs::read_to_string(&p)?;
        // A kill mid-write can leave a partial last line; it is skipped.
        let messages = text
            .lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect();
        let file = OpenOptions::new().append(true).open(p)?;
        Ok(Session {
            id: id.to_owned(),
            file,
            messages,
        })
    }

    /// Appends one message, on disk first.
    pub fn push(&mut self, message: Value) -> io::Result<()> {
        let mut line = serde_json::to_vec(&message)?;
        line.push(b'\n');
        self.file.write_all(&line)?;
        self.messages.push(message);
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
