//! Files the user attaches to a message, put in `attachments/` in the bot's folder, and a
//! chat written out as Markdown.

use crate::{memory, paths};
use serde_json::Value;
use std::io::{self, Write};

/// Most bytes of one attached file.
pub const MAX_BYTES: usize = 25 * 1024 * 1024;
/// Most files one window may be sending at a time.
pub const MAX_UPLOADS: usize = 4;
const DIR: &str = "attachments";

/// A file on its way in, part by part.
pub struct Upload {
    pub bot: String,
    pub name: String,
    pub bytes: Vec<u8>,
}

/// A name the file may have in the folder: one part, not hidden.
pub fn check_name(name: &str) -> Result<(), String> {
    if name.is_empty()
        || name.len() > 100
        || name.starts_with('.')
        || name.contains(['/', '\0'])
        || name.chars().any(char::is_control)
    {
        return Err("`name` must be a file name of 1 to 100 bytes, not starting with a dot".into());
    }
    Ok(())
}

/// Writes the file under `name`, or `1-name`, `2-name`… when taken, and gives its path in the
/// bot's folder. A link the bot left in the way is never followed (`sys::Dir`).
pub fn save(bot: &str, name: &str, bytes: &[u8]) -> io::Result<String> {
    let dir = memory::made(bot, DIR)?;
    for n in 0..1000 {
        let file = if n == 0 {
            name.to_owned()
        } else {
            format!("{n}-{name}")
        };
        match dir.create(&file) {
            Ok(mut out) => {
                if let Err(e) = out.write_all(bytes) {
                    // A part of the file must not keep the name.
                    let _ = dir.remove(&file);
                    return Err(e);
                }
                return Ok(format!("{DIR}/{file}"));
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::other(format!("too many files named {name}")))
}

/// Standard base64, padded or not; `None` if it is not.
pub fn base64(text: &str) -> Option<Vec<u8>> {
    let text = text.trim_end_matches('=');
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let (mut bits, mut held) = (0u32, 0u32);
    for c in text.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        };
        bits = (bits << 6) | u32::from(v);
        held += 6;
        if held >= 8 {
            held -= 8;
            out.push((bits >> held) as u8);
        }
    }
    // A lone last character holds no whole byte.
    (held < 6).then_some(out)
}

/// Every line of `text` quoted, so nothing in it reads as a heading of the export.
fn quoted(text: &str) -> String {
    text.lines().map(|line| format!("> {line}\n")).collect()
}

/// The chat as Markdown: what the user wrote, the replies, and which tools ran. What each
/// side wrote is quoted, so a reply cannot pass itself off as the user.
pub fn markdown(name: &str, history: &[Value]) -> String {
    let mut out = format!("# {}\n", name.replace(['\n', '\r'], " "));
    let mut reply = String::new();
    let flush = |out: &mut String, reply: &mut String| {
        if !reply.is_empty() {
            out.push_str("\n## Bot\n\n");
            out.push_str(&quoted(reply));
            reply.clear();
        }
    };
    for row in history {
        let entry = &row["entry"];
        match entry["type"].as_str().unwrap_or("") {
            "text" => reply.push_str(entry["delta"].as_str().unwrap_or("")),
            "prompt" => {
                flush(&mut out, &mut reply);
                out.push_str("\n## You\n\n");
                out.push_str(&quoted(entry["text"].as_str().unwrap_or("")));
            }
            "tool_start" => {
                flush(&mut out, &mut reply);
                let tool = entry["tool"].as_str().unwrap_or("a tool");
                out.push_str(&format!("\n_Used `{}`_\n", tool.replace('`', "")));
            }
            "turn_end" if entry["reason"] != "done" => {
                flush(&mut out, &mut reply);
                let what = if entry["reason"] == "aborted" {
                    "was stopped"
                } else {
                    "failed"
                };
                out.push_str(&format!("\n_The reply {what}._\n"));
            }
            _ => {}
        }
    }
    flush(&mut out, &mut reply);
    out
}

/// Writes the chat to `exports/` in the service's home and gives the path.
pub fn export(bot: &str, name: &str, history: &[Value], now_ms: u64) -> io::Result<String> {
    let dir = paths::home().join("exports");
    crate::sys::create_private_dir(&dir)?;
    let path = dir.join(format!("bate-chat-{bot}-{now_ms}.md"));
    std::fs::write(&path, markdown(name, history))?;
    Ok(path.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn base64_decodes_and_refuses_what_is_not() {
        assert_eq!(base64("aGVsbG8=").unwrap(), b"hello");
        assert_eq!(base64("aGVsbG8").unwrap(), b"hello");
        assert_eq!(base64("").unwrap(), b"");
        assert_eq!(base64("/+8=").unwrap(), [0xff, 0xef]);
        assert_eq!(base64("aGVsbG8*"), None);
        assert_eq!(base64("a"), None);
    }

    #[test]
    fn only_plain_names_are_taken() {
        assert!(check_name("photo 1.png").is_ok());
        for bad in ["", ".env", "../x", "a/b", "a\nb", &"x".repeat(101)] {
            assert!(check_name(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn a_chat_reads_as_markdown() {
        let rows: Vec<Value> = [
            json!({"type": "prompt", "nonce": "n", "text": "hi"}),
            json!({"type": "thinking", "delta": "hmm"}),
            json!({"type": "text", "delta": "Hel"}),
            json!({"type": "thinking", "delta": "more"}),
            json!({"type": "text", "delta": "lo\n## You\nfake"}),
            json!({"type": "tool_start", "call": "c", "tool": "read", "args": {}}),
            json!({"type": "turn_end", "reason": "aborted"}),
        ]
        .into_iter()
        .map(|entry| json!({"entry": entry}))
        .collect();
        assert_eq!(
            markdown("Ada\n# B", &rows),
            "# Ada # B\n\n## You\n\n> hi\n\n## Bot\n\n> Hello\n> ## You\n> fake\n\n_Used `read`_\n\n_The reply was stopped._\n"
        );
    }
}
