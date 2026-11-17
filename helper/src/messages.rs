//! Reads the user's Messages history, `~/Library/Messages/chat.db`, which only a program with
//! Full Disk Access may open. That program is this one: the service must never hold the
//! permission, since every program it starts would inherit it. The file is opened read-only.

use rusqlite::{Connection, OpenFlags, params};
use std::fs::File;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

pub const NO_ACCESS: &str = "Bate Helper is not allowed to read Messages. The user must allow it in System Settings > Privacy & Security > Full Disk Access (add Bate Helper with + if it is not listed).";
/// Newest messages a search looks through. Text is often only in `attributedBody`, which SQL
/// cannot search, so each is decoded here.
// shortcut: older messages are not searched; index the history once searches need them.
const SCAN: i64 = 20_000;
/// Longest text shown for one message, in characters.
const MAX_TEXT: usize = 1000;

fn home() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
}

pub fn chat_db() -> PathBuf {
    home().join("Library/Messages/chat.db")
}

/// Whether this program has Full Disk Access: it can open the TCC database, which exists on
/// every Mac, and no other permission lets a program do that.
pub fn full_disk_access() -> bool {
    File::open(home().join("Library/Application Support/com.apple.TCC/TCC.db")).is_ok()
}

/// The text of a message's `attributedBody`, a typedstream: the first `NSString` is followed
/// by `+`, the text's length (one byte, or `0x81` and two bytes, or `0x82` and four, little
/// endian) and the UTF-8 text.
fn attributed_text(blob: &[u8]) -> Option<String> {
    const NS_STRING: &[u8] = b"NSString";
    let after = blob.windows(NS_STRING.len()).position(|w| w == NS_STRING)? + NS_STRING.len();
    let plus = after + blob[after..].iter().take(16).position(|b| *b == b'+')?;
    let rest = &blob[plus + 1..];
    let (len, rest) = match *rest.first()? {
        0x81 => (
            u16::from_le_bytes(rest.get(1..3)?.try_into().ok()?) as usize,
            &rest[3..],
        ),
        0x82 => (
            u32::from_le_bytes(rest.get(1..5)?.try_into().ok()?) as usize,
            &rest[5..],
        ),
        n if n < 0x80 => (n as usize, &rest[1..]),
        _ => return None,
    };
    Some(String::from_utf8_lossy(rest.get(..len)?).into_owned())
}

/// What the user wrote, whichever column holds it; an attachment shows as `[attachment]`.
fn text_of(text: Option<String>, body: Option<Vec<u8>>) -> Option<String> {
    let text = text
        .filter(|t| !t.is_empty())
        .or_else(|| attributed_text(&body?))?;
    let text = text.replace('\u{fffc}', "[attachment]");
    let text = text.trim();
    let mut shown: String = text.chars().take(MAX_TEXT).collect();
    if shown.len() < text.len() {
        shown.push('…');
    }
    (!shown.is_empty()).then_some(shown)
}

/// The newest `limit` messages, those with `query` in their text (any case) if given, and of
/// the conversation `with` (as Messages names it: a phone number like `+4915112345678`, or an
/// address) if given. One per line, newest first.
// shortcut: `with` must match the conversation's name exactly, ignoring case, spaces and dashes.
pub fn read(
    db: &Path,
    query: Option<&str>,
    with: Option<&str>,
    limit: u32,
) -> Result<String, String> {
    match File::open(db) {
        Ok(_) => {}
        Err(e) if e.kind() == ErrorKind::PermissionDenied => return Err(NO_ACCESS.into()),
        Err(e) if e.kind() == ErrorKind::NotFound => {
            return Err(format!(
                "There is no Messages history on this Mac: {} does not exist.",
                db.display()
            ));
        }
        Err(e) => return Err(format!("Messages' history cannot be opened: {e}")),
    }
    let failed = |e: rusqlite::Error| format!("Messages' history cannot be read: {e}");
    let conn = Connection::open_with_flags(
        db,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(failed)?;
    conn.busy_timeout(std::time::Duration::from_secs(2))
        .map_err(failed)?;
    // Dates are nanoseconds since 2001-01-01.
    let mut stmt = conn
        .prepare(
            "SELECT datetime(m.date / 1000000000 + 978307200, 'unixepoch', 'localtime'),
                    m.is_from_me, h.id, c.chat_identifier, c.display_name, m.text, m.attributedBody
             FROM message m
             LEFT JOIN handle h ON h.ROWID = m.handle_id
             LEFT JOIN chat_message_join j ON j.message_id = m.ROWID
             LEFT JOIN chat c ON c.ROWID = j.chat_id
             WHERE ?1 IS NULL OR c.chat_identifier = ?1 COLLATE NOCASE
             ORDER BY m.date DESC LIMIT ?2",
        )
        .map_err(failed)?;
    let with = with.map(|w| w.replace([' ', '-', '(', ')'], ""));
    let scan = if query.is_some() {
        SCAN
    } else {
        i64::from(limit)
    };
    let mut rows = stmt.query(params![with, scan]).map_err(failed)?;
    let needle = query.map(str::to_lowercase);
    let mut out = String::new();
    let mut found = 0;
    while found < limit {
        let Some(row) = rows.next().map_err(failed)? else {
            break;
        };
        let Some(text) = text_of(row.get(5).ok().flatten(), row.get(6).ok().flatten()) else {
            continue;
        };
        if needle
            .as_ref()
            .is_some_and(|n| !text.to_lowercase().contains(n))
        {
            continue;
        }
        let col = |i: usize| row.get::<_, Option<String>>(i).ok().flatten();
        let who = if row.get::<_, i64>(1).unwrap_or(0) != 0 {
            "me".to_owned()
        } else {
            col(2).unwrap_or_else(|| "unknown".into())
        };
        let chat = col(4).filter(|n| !n.is_empty()).or_else(|| col(3));
        // A conversation asked for need not be named on each line.
        let chat = chat
            .filter(|_| with.is_none())
            .map(|c| format!("[{c}] "))
            .unwrap_or_default();
        out.push_str(&format!(
            "{} {chat}{who}: {}\n",
            col(0).unwrap_or_default(),
            text.replace('\n', "\n  ")
        ));
        found += 1;
    }
    Ok(if out.is_empty() {
        "No messages found.".into()
    } else {
        out
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    /// An `attributedBody` as Messages writes it, around `text` with its length written as `len`.
    fn typedstream(len: &[u8], text: &str) -> Vec<u8> {
        let mut b = b"\x04\x0bstreamtyped\x81\xe8\x03\x84\x01@\x84\x84\x84\x12NSAttributedString\0\x84\x84\x08NSObject\0\x85\x92\x84\x84\x84\x08NSString\x01\x94\x84\x01+".to_vec();
        b.extend(len);
        b.extend(text.as_bytes());
        b.extend(b"\x86\x84\x84\x84\x0cNSDictionary\0\x84\x84\x84\x0bNSDictionary");
        b
    }

    fn blob(text: &str) -> Vec<u8> {
        match text.len() {
            n if n < 0x80 => typedstream(&[n as u8], text),
            n => typedstream(&[&[0x81][..], &(n as u16).to_le_bytes()].concat(), text),
        }
    }

    /// A history of the shape Messages keeps: a text, an `attributedBody` only, a long one, an
    /// attachment, a group, and an older one with another person.
    fn fixture(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("helper-chatdb-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("chat.db");
        let db = Connection::open(&path).unwrap();
        db.execute_batch(
            "CREATE TABLE handle(ROWID INTEGER PRIMARY KEY, id TEXT);
             CREATE TABLE chat(ROWID INTEGER PRIMARY KEY, chat_identifier TEXT, display_name TEXT);
             CREATE TABLE message(ROWID INTEGER PRIMARY KEY, date INTEGER, is_from_me INTEGER,
                                  handle_id INTEGER, text TEXT, attributedBody BLOB);
             CREATE TABLE chat_message_join(chat_id INTEGER, message_id INTEGER);
             INSERT INTO handle VALUES (1, '+4915112345678'), (2, 'anna@example.com');
             INSERT INTO chat VALUES (1, '+4915112345678', ''), (2, 'anna@example.com', ''),
                                     (3, 'chat123', 'Climbing');",
        )
        .unwrap();
        let long = "Ä".repeat(200);
        let rows = [
            (1_i64, 0, 2, Some("old news from Anna"), None, 2),
            (2, 0, 1, Some("Lunch at noon?"), None, 1),
            (3, 1, 1, None, Some(blob("Sounds good, LUNCH it is")), 1),
            (4, 0, 1, None, Some(blob(&long)), 1),
            (5, 0, 1, None, Some(blob("\u{fffc}")), 1),
            (6, 0, 2, Some("see you at the wall"), None, 3),
            (7, 1, 0, Some("line one\nline two"), None, 3),
        ];
        for (id, from_me, handle, text, body, chat) in rows {
            db.execute(
                "INSERT INTO message VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![id, id * 1_000_000_000 * 3600, from_me, handle, text, body],
            )
            .unwrap();
            db.execute(
                "INSERT INTO chat_message_join VALUES (?1, ?2)",
                params![chat, id],
            )
            .unwrap();
        }
        path
    }

    fn lines(out: &str) -> Vec<&str> {
        out.lines().collect()
    }

    #[test]
    fn the_text_of_an_attributed_body_is_found_whatever_its_length() {
        assert_eq!(attributed_text(&blob("hi")).as_deref(), Some("hi"));
        assert_eq!(
            attributed_text(&blob("grüße 👋")).as_deref(),
            Some("grüße 👋")
        );
        let long = "x".repeat(300);
        assert_eq!(attributed_text(&blob(&long)), Some(long));
        assert_eq!(
            attributed_text(&typedstream(&[0x82, 3, 0, 0, 0], "abc")).as_deref(),
            Some("abc")
        );
        // Not a typedstream, or a cut one: nothing, not a panic.
        assert_eq!(attributed_text(b"plain"), None);
        assert_eq!(attributed_text(&blob("hello")[..70]), None);
    }

    #[test]
    fn the_newest_messages_come_first_from_text_and_attributed_body_alike() {
        let db = fixture("recent");
        let out = read(&db, None, None, 4).unwrap();
        let got = lines(&out);
        // Message 7 spans two lines; the attachment-only message 5 shows as one.
        assert_eq!(got.len(), 5, "{out}");
        assert!(got[0].ends_with("[Climbing] me: line one"), "{out}");
        assert_eq!(got[1], "  line two");
        assert!(
            got[2].ends_with("[Climbing] anna@example.com: see you at the wall"),
            "{out}"
        );
        assert!(
            got[3].ends_with("[+4915112345678] +4915112345678: [attachment]"),
            "{out}"
        );
        assert!(got[4].ends_with(&format!(": {}", "Ä".repeat(200))), "{out}");
        let all = read(&db, None, None, 50).unwrap();
        assert!(all.contains("me: Sounds good, LUNCH it is"), "{all}");
        assert_eq!(lines(&all).len(), 8, "{all}");
        // A long message is cut.
        let cut = text_of(Some("x".repeat(MAX_TEXT + 1)), None).unwrap();
        assert_eq!(cut.chars().count(), MAX_TEXT + 1);
        assert!(cut.ends_with('…'));
    }

    #[test]
    fn a_conversation_and_a_search_narrow_it_down() {
        let db = fixture("narrow");
        let with = read(&db, None, Some("+49 151-1234 5678"), 50).unwrap();
        assert_eq!(lines(&with).len(), 4, "{with}");
        assert!(!with.contains("[+49"), "{with}");
        let anna = read(&db, None, Some("ANNA@example.com"), 50).unwrap();
        assert_eq!(lines(&anna).len(), 1, "{anna}");
        // Found in `text` and in `attributedBody`, in any case, only where asked.
        let found = read(&db, Some("lunch"), None, 50).unwrap();
        assert_eq!(lines(&found).len(), 2, "{found}");
        assert!(
            found.contains("Lunch at noon?") && found.contains("Sounds good"),
            "{found}"
        );
        let found = read(&db, Some("lunch"), Some("anna@example.com"), 50).unwrap();
        assert_eq!(found, "No messages found.");
        assert_eq!(
            read(&db, Some("lunch"), None, 1).unwrap().lines().count(),
            1
        );
    }

    #[test]
    fn the_history_is_opened_read_only_and_its_absence_or_refusal_is_told() {
        let db = fixture("guard");
        let before = fs::read(&db).unwrap();
        read(&db, Some("x"), None, 5).unwrap();
        assert_eq!(fs::read(&db).unwrap(), before);
        let none = db.with_file_name("none.db");
        let e = read(&none, None, None, 5).unwrap_err();
        assert!(e.contains("no Messages history"), "{e}");
        // Without Full Disk Access the open fails with a refusal, not a missing file.
        fs::set_permissions(&db, fs::Permissions::from_mode(0o000)).unwrap();
        assert_eq!(read(&db, None, None, 5).unwrap_err(), NO_ACCESS);
        fs::set_permissions(&db, fs::Permissions::from_mode(0o600)).unwrap();
        let junk = db.with_file_name("junk.db");
        fs::write(&junk, "not a database").unwrap();
        assert!(
            read(&junk, None, None, 5)
                .unwrap_err()
                .contains("cannot be read")
        );
    }
}
