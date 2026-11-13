//! The bug-report archive: the service's logs and settings with secrets removed.
//!
//! Settings never carry a connector's `env` values, header values, arguments or
//! URL query. Logs are scrubbed of every such stored value, and of whatever
//! follows a word that announces a secret (`token=…`, `Authorization: …`).
//! Conversations are not included.

use crate::store::Store;
use crate::{paths, sys};
use proto::connectors::Transport;
use serde_json::{Value, json};
use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

const REMOVED: &str = "[removed]";
/// A stored value shorter than this is not searched for in logs: it would match ordinary text.
const MIN_SECRET: usize = 4;
/// What follows one of these and a `:` or `=` is removed, as is the word after `bearer`.
/// `key` also covers `api_key`, `apikey` and `secret_key`.
const NAMES: [&str; 7] = [
    "authorization",
    "password",
    "passwd",
    "secret",
    "bearer",
    "token",
    "key",
];

/// Writes `reports/bate-report-<now_ms>.tar.gz` under the service's home and returns its path.
// shortcut: runs on the core's thread (about 20 MiB of logs at most); move it off if it is ever slow.
pub fn export(store: &Store, now_ms: u64) -> io::Result<PathBuf> {
    let db = io::Error::other;
    let mut secrets: Vec<String> = std::env::var("BATE_MODEL_API_KEY").into_iter().collect();
    let mut bots = Vec::new();
    for bot in store.bots().map_err(db)? {
        let mut connectors = Vec::new();
        for c in store.connectors(&bot.id).map_err(db)? {
            let transport = match &c.transport {
                Transport::Stdio { command, args, env } => {
                    secrets.extend(args.iter().chain(env.values()).cloned());
                    json!({"type": "stdio", "command": command, "args": args.len(),
                        "env": env.keys().collect::<Vec<_>>()})
                }
                Transport::Http { url, headers } => {
                    let (address, query) = url.split_once('?').unwrap_or((url, ""));
                    for value in headers.values() {
                        secrets.push(value.clone());
                        // `Bearer abc`: the token alone may be logged too.
                        secrets.extend(value.split_whitespace().map(str::to_owned));
                    }
                    for pair in query.split('&') {
                        secrets.push(pair.split_once('=').map_or(pair, |(_, v)| v).to_owned());
                    }
                    json!({"type": "http", "url": address,
                        "headers": headers.keys().collect::<Vec<_>>()})
                }
            };
            connectors.push(json!({"name": c.name, "enabled": c.enabled, "transport": transport}));
        }
        let routines: Vec<Value> = store
            .routines(&bot.id)
            .map_err(db)?
            .into_iter()
            .map(|r| {
                json!({"id": r.id, "enabled": r.enabled, "schedule": r.schedule,
                    "anchor": r.anchor, "nextRunMs": r.next_run_ms, "lastRun": r.last_run})
            })
            .collect();
        bots.push(json!({"id": bot.id, "createdAt": bot.created_at,
            "routines": routines, "connectors": connectors}));
    }
    let settings = json!({
        "appd": env!("CARGO_PKG_VERSION"),
        "engineProtocol": proto::PROTOCOL,
        "windowProtocol": proto::WINDOW_PROTOCOL,
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "bots": bots,
    });
    secrets.retain(|s| s.len() >= MIN_SECRET);
    // Longest first, so a secret that contains another one goes whole.
    secrets.sort_by_key(|s| std::cmp::Reverse(s.len()));

    let reports = paths::home().join("reports");
    sys::create_private_dir(&reports)?;
    let staging = reports.join(format!(".staging-{}", sys::random_hex(4)));
    let archive = reports.join(format!("bate-report-{now_ms}.tar.gz"));
    let built = build(&staging, &archive, &settings, &secrets);
    let _ = fs::remove_dir_all(&staging);
    built.map(|()| archive)
}

fn build(staging: &Path, archive: &Path, settings: &Value, secrets: &[String]) -> io::Result<()> {
    fs::create_dir_all(staging.join("logs"))?;
    let pretty = serde_json::to_string_pretty(settings).map_err(io::Error::other)?;
    fs::write(staging.join("settings.json"), pretty)?;
    for entry in fs::read_dir(paths::logs())? {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            let text = String::from_utf8_lossy(&fs::read(entry.path())?).into_owned();
            fs::write(
                staging.join("logs").join(entry.file_name()),
                scrub(&text, secrets),
            )?;
        }
    }
    let tar = Command::new("/usr/bin/tar")
        .arg("-czf")
        .arg(archive)
        .arg("-C")
        .arg(staging)
        .args(["settings.json", "logs"])
        .status()?;
    if !tar.success() {
        return Err(io::Error::other(format!("tar: {tar}")));
    }
    fs::set_permissions(archive, fs::Permissions::from_mode(0o600))
}

/// `text` without the known `secrets` and without what follows a word that announces one.
fn scrub(text: &str, secrets: &[String]) -> String {
    let mut text = text.to_owned();
    for secret in secrets {
        text = text.replace(secret.as_str(), REMOVED);
    }
    // ASCII lower case keeps every byte offset.
    let lower = text.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    let mut out = String::with_capacity(text.len());
    let (mut copied, mut i) = (0, 0);
    while i < bytes.len() {
        let Some(name) = NAMES.iter().find(|n| lower[i..].starts_with(**n)) else {
            // To the next character, not into the middle of one.
            i += 1;
            while !lower.is_char_boundary(i) {
                i += 1;
            }
            continue;
        };
        i += name.len();
        let skip = |mut at: usize, set: &[u8]| {
            while at < bytes.len() && set.contains(&bytes[at]) {
                at += 1;
            }
            at
        };
        let mut start = skip(i, b"\"' ");
        if *name == "bearer" {
            // The word after it; `bearer` with nothing after a space is left alone.
            if start == i {
                continue;
            }
        } else {
            if start >= bytes.len() || !b":=".contains(&bytes[start]) {
                continue;
            }
            start = skip(start + 1, b"\"' ");
            // `Authorization: Bearer abc`: keep the scheme, remove the token.
            if lower[start..].starts_with("bearer ") {
                start = skip(start + 6, b" ");
            }
        }
        let end = bytes[start..]
            .iter()
            .position(|b| b" \t\r\n\"'&,;}".contains(b))
            .map_or(bytes.len(), |n| start + n);
        if end > start && &text[start..end] != REMOVED {
            out.push_str(&text[copied..start]);
            out.push_str(REMOVED);
            copied = end;
        }
        i = end.max(i);
    }
    out.push_str(&text[copied..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_secrets_and_announced_ones_are_removed_and_the_rest_is_kept() {
        let secrets = ["abc-long-secret".to_owned(), "abc-long".to_owned()];
        let cases = [
            (
                "connector x: got abc-long-secret back",
                "connector x: got [removed] back",
            ),
            (
                "Authorization: Bearer eyJhbGciOi.x.y",
                "Authorization: Bearer [removed]",
            ),
            ("header bearer tok_123 sent", "header bearer [removed] sent"),
            (
                "GET /mcp?api_key=k1&page=2",
                "GET /mcp?api_key=[removed]&page=2",
            ),
            (
                r#"{"access_token": "t0k", "n": 1}"#,
                r#"{"access_token": "[removed]", "n": 1}"#,
            ),
            ("PASSWORD = hunter2;", "PASSWORD = [removed];"),
            ("client_secret='s3'", "client_secret='[removed]'"),
            // Not secrets: no `:` or `=` right after the word, and ordinary text.
            ("used 512 tokens: fine", "used 512 tokens: fine"),
            (
                "the key to the café is patience",
                "the key to the café is patience",
            ),
            ("bot 1a2b: turn ended", "bot 1a2b: turn ended"),
        ];
        for (text, expected) in cases {
            assert_eq!(scrub(text, &secrets), expected, "{text}");
        }
    }
}
