//! The bug-report archive: the service's logs and settings with secrets removed.
//!
//! Settings carry no connector value that can hold a secret: only the names of
//! `env` and header entries, the number of arguments, and a URL's scheme and host.
//! Logs are scrubbed of every such stored value, of what follows a word that
//! announces a secret (`token=…`, `Authorization: …`), and of PEM blocks.
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
/// A whole argument or a piece of a URL's path shorter than this is taken for an ordinary word.
const MIN_WORD: usize = 12;
/// What follows one of these and a `:` or `=` is removed, as is the word after `bearer` and
/// after a flag that ends in one (`--api-key value`). Longer names first: the first match wins.
/// `key` also covers `api_key`, `apikey` and `secret_key`; `auth` covers `oauth`.
const NAMES: [&str; 13] = [
    "authorization",
    "credential",
    "signature",
    "password",
    "passwd",
    "cookie",
    "secret",
    "bearer",
    "token",
    "auth",
    "pwd",
    "sig",
    "key",
];
/// After these the value runs to the end of the line: it has a scheme and may have spaces.
const WHOLE_LINE: [&str; 3] = ["authorization", "auth", "cookie"];

/// Writes `reports/bate-report-<now_ms>.tar.gz` under the service's home and returns its path.
// shortcut: runs on the core's thread (about 20 MiB of logs at most); move it off if it is ever slow.
pub fn export(store: &Store, now_ms: u64) -> io::Result<PathBuf> {
    let db = io::Error::other;
    let mut secrets: Vec<String> = std::env::var("BATE_MODEL_API_KEY").into_iter().collect();
    if let Ok(Some((_, Some(key)))) = store.provider() {
        secrets.push(key);
    }
    let mut bots = Vec::new();
    for bot in store.bots().map_err(db)? {
        let mut connectors = Vec::new();
        for c in store.connectors(&bot.id).map_err(db)? {
            let transport = match &c.transport {
                Transport::Stdio { command, args, env } => {
                    for value in env.values() {
                        secrets.push(value.clone());
                        secrets.extend(value.lines().map(str::to_owned));
                    }
                    for arg in args {
                        // `--key=abc`: the value alone may be logged; a short bare argument is a word.
                        match arg.split_once('=') {
                            Some((_, value)) => secrets.extend([arg.clone(), value.to_owned()]),
                            None if arg.len() >= MIN_WORD => secrets.push(arg.clone()),
                            None => {}
                        }
                    }
                    json!({"type": "stdio", "command": command, "args": args.len(),
                        "env": env.keys().collect::<Vec<_>>()})
                }
                Transport::Http { url, headers } => {
                    for value in headers.values() {
                        secrets.push(value.clone());
                        // `Bearer abc`, `sid=abc; t=x`: a part alone may be logged too.
                        secrets.extend(
                            value
                                .split([' ', '\t', ';', ',', '='])
                                .filter(|part| {
                                    !["bearer", "basic"]
                                        .contains(&part.to_ascii_lowercase().as_str())
                                })
                                .map(str::to_owned),
                        );
                    }
                    let (address, parts) = url_parts(url);
                    secrets.push(url.clone());
                    secrets.extend(parts);
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
    secrets.dedup();

    let reports = paths::home().join("reports");
    sys::create_private_dir(&reports)?;
    let staging = reports.join(format!(".staging-{}", sys::random_hex(4)));
    let archive = reports.join(format!("bate-report-{now_ms}.tar.gz"));
    let built = build(&staging, &archive, &settings, &secrets);
    let _ = fs::remove_dir_all(&staging);
    if built.is_err() {
        let _ = fs::remove_file(&archive);
    }
    built.map(|()| archive)
}

/// `scheme://host[:port]` of `url`, and the parts of the rest that may be secret: user and
/// password, query values, and long pieces of the path.
fn url_parts(url: &str) -> (String, Vec<String>) {
    let (scheme, rest) = url.split_once("://").unwrap_or(("", url));
    let (authority, tail) = rest.split_at(rest.find(['/', '?', '#']).unwrap_or(rest.len()));
    let (userinfo, host) = authority.rsplit_once('@').unwrap_or(("", authority));
    let mut parts: Vec<String> = userinfo.split(':').map(str::to_owned).collect();
    let (path, query) = tail.split_once('?').unwrap_or((tail, ""));
    parts.extend(
        path.split(['/', '#'])
            .filter(|piece| piece.len() >= MIN_WORD)
            .map(str::to_owned),
    );
    for pair in query.split(['&', '#']) {
        parts.push(pair.split_once('=').map_or(pair, |(_, v)| v).to_owned());
    }
    (format!("{scheme}://{host}"), parts)
}

fn build(staging: &Path, archive: &Path, settings: &Value, secrets: &[String]) -> io::Result<()> {
    fs::create_dir_all(staging.join("logs"))?;
    let pretty = serde_json::to_string_pretty(settings).map_err(io::Error::other)?;
    // Scrubbed too: a routine's last error is free text.
    fs::write(staging.join("settings.json"), scrub(&pretty, secrets))?;
    for entry in fs::read_dir(paths::logs())? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let bytes = match fs::read(entry.path()) {
            Ok(bytes) => bytes,
            // Rotated away since the folder was listed.
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e),
        };
        fs::write(
            staging.join("logs").join(entry.file_name()),
            scrub(&String::from_utf8_lossy(&bytes), secrets),
        )?;
    }
    let tar = Command::new("/usr/bin/tar")
        // No `._` files with extended attributes.
        .env("COPYFILE_DISABLE", "1")
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

/// `text` without the known `secrets`, without PEM blocks and without what follows a word that
/// announces a secret.
fn scrub(text: &str, secrets: &[String]) -> String {
    let mut text = text.to_owned();
    for secret in secrets {
        text = text.replace(secret.as_str(), REMOVED);
    }
    without_pem(&text)
        .split_inclusive('\n')
        .map(scrub_line)
        .collect()
}

/// `text` with each PEM block (`-----BEGIN …` to the end of its `-----END` line) removed.
fn without_pem(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(begin) = rest.find("-----BEGIN") {
        out.push_str(&rest[..begin]);
        out.push_str(REMOVED);
        let block = &rest[begin..];
        // A block that never ends goes to the end.
        rest = block.find("-----END").map_or("", |end| {
            let last = &block[end..];
            &last[last.find('\n').unwrap_or(last.len())..]
        });
    }
    out.push_str(rest);
    out
}

fn scrub_line(line: &str) -> String {
    // ASCII lower case keeps every byte offset.
    let lower = line.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    let line_end = line.trim_end_matches(['\r', '\n']).len();
    let skip = |mut at: usize, set: &[u8]| {
        while at < line_end && set.contains(&bytes[at]) {
            at += 1;
        }
        at
    };
    let mut out = String::with_capacity(line.len());
    let (mut copied, mut i) = (0, 0);
    while i < line_end {
        let Some(name) = NAMES.iter().find(|n| lower[i..].starts_with(**n)) else {
            // To the next character, not into the middle of one.
            i += 1;
            while !lower.is_char_boundary(i) {
                i += 1;
            }
            continue;
        };
        // `--api-key value`: a flag gives its value after a space.
        let word = lower[..i].rfind([' ', '\t']).map_or(0, |n| n + 1);
        let spaced = *name == "bearer" || bytes[word] == b'-';
        i += name.len();
        // Quotes may be escaped (`\"token\":\"abc\"`).
        let after = skip(i, b"\"'\\ \t");
        let start = if after < line_end && b":=".contains(&bytes[after]) {
            skip(after + 1, b"\"'\\ \t")
        } else if spaced && after > i {
            after
        } else {
            continue;
        };
        let quote = bytes[..start]
            .last()
            .copied()
            .filter(|b| b"\"'".contains(b));
        let end = match quote {
            // To the closing quote, or the backslash that escapes it.
            Some(q) => bytes[start..line_end]
                .iter()
                .position(|b| *b == q || *b == b'\\')
                .map_or(line_end, |n| start + n),
            None if WHOLE_LINE.contains(name) => line_end,
            None => bytes[start..line_end]
                .iter()
                .position(|b| b" \t&\"'".contains(b))
                .map_or(line_end, |n| start + n),
        };
        if end > start && &line[start..end] != REMOVED {
            out.push_str(&line[copied..start]);
            out.push_str(REMOVED);
            copied = end;
        }
        i = end.max(i);
    }
    out.push_str(&line[copied..]);
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
                "Authorization: [removed]",
            ),
            (
                "Authorization: Basic dXNlcjpwYXNz",
                "Authorization: [removed]",
            ),
            (
                "auth: token ghp_x and more\nnext line",
                "auth: [removed]\nnext line",
            ),
            ("Cookie: sid=abc; t=x", "Cookie: [removed]"),
            ("header bearer tok_123 sent", "header bearer [removed] sent"),
            (
                "GET /mcp?api_key=k1&page=2",
                "GET /mcp?api_key=[removed]&page=2",
            ),
            (
                r#"{"access_token": "t0k", "n": 1}"#,
                r#"{"access_token": "[removed]", "n": 1}"#,
            ),
            (
                r#"sent {\"token\":\"abc\"} ok"#,
                r#"sent {\"token\":\"[removed]\"} ok"#,
            ),
            (
                r#"password="correct horse battery" ok"#,
                r#"password="[removed]" ok"#,
            ),
            ("password=a,b;c next", "password=[removed] next"),
            ("token:\tabc", "token:\t[removed]"),
            ("PASSWORD = hunter2", "PASSWORD = [removed]"),
            ("client_secret='s3'", "client_secret='[removed]'"),
            (
                "ran tool --api-key sk-x --port 80",
                "ran tool --api-key [removed] --port 80",
            ),
            ("pwd=é1ñ sig=ü2 done", "pwd=[removed] sig=[removed] done"),
            (
                "key is -----BEGIN PRIVATE KEY-----\nMIIE\nxyz\n-----END PRIVATE KEY-----\nafter",
                "key is [removed]\nafter",
            ),
            // Not secrets: no `:` or `=` right after the word, and ordinary text.
            ("used 512 tokens: fine", "used 512 tokens: fine"),
            (
                "the key to the café is patience",
                "the key to the café is patience",
            ),
            ("bot 1a2b: turn ended\r\n", "bot 1a2b: turn ended\r\n"),
        ];
        for (text, expected) in cases {
            assert_eq!(scrub(text, &secrets), expected, "{text}");
        }
    }

    #[test]
    fn a_url_keeps_only_its_scheme_and_host() {
        let (address, parts) = url_parts(
            "https://user:pa55word@example.com:8443/mcp/s/0123456789abcdef/mcp?sig=q1w2&x=1#frag",
        );
        assert_eq!(address, "https://example.com:8443");
        for secret in ["user", "pa55word", "0123456789abcdef", "q1w2"] {
            assert!(parts.iter().any(|p| p == secret), "{secret} in {parts:?}");
        }
        assert!(!parts.iter().any(|p| p == "mcp"), "{parts:?}");
    }
}
