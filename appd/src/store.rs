//! All state in one SQLite database, `app.db`, in write-ahead mode, written
//! only by the service. Each change is its own committed transaction, so a
//! row is on disk before the service acts on it.

use std::path::Path;

use crate::keychain;
use crate::oauth::Tokens;
use crate::sys::{log, random_hex};
use proto::connectors::Connector;
use proto::permissions::{Grant, Kind, Level};
use proto::projects::Project;
use proto::routines::{Routine, RoutineRun};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use serde_json::Value;

pub type Result<T, E = rusqlite::Error> = std::result::Result<T, E>;

pub struct Store(Connection);

/// Whose file copies they are: what is kept is bounded for each, and goes with it.
#[derive(Clone, Copy)]
pub enum Owner<'a> {
    Bot(&'a str),
    /// A window's change to a project's note, which no bot made.
    Project(&'a str),
}

/// A file as it was before a change (`file_copies`, see "Undo" in PROTOCOL.md).
pub struct Kept {
    /// What the engine named it, or the service for a copy of its own.
    pub id: String,
    /// The conversation whose tool line made it.
    pub conversation: String,
    /// Full path.
    pub path: String,
    /// `None`: there was no file.
    pub earlier: Option<Vec<u8>>,
    /// Hash of what the change left; none for a change the window made.
    pub after: Option<String>,
    /// Who changed it: `bot` or `user`.
    pub who: &'static str,
    pub at: u64,
    /// Output only: the bot whose copy it is, and whether the change was undone.
    pub bot: Option<String>,
    pub undone: bool,
}

/// What a note's earlier version is, for a list.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Version {
    pub id: String,
    pub at_ms: u64,
    pub bytes: u64,
    pub by: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BotRow {
    pub id: String,
    pub name: String,
    pub created_at: u64,
}

/// One of a bot's conversations. The engine's session it works in is the store's own.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Conversation {
    pub id: String,
    pub bot: String,
    pub created_at: u64,
    /// When a prompt last went into it, in Unix milliseconds.
    pub touched_at: u64,
    /// Made from the first prompt once the bot had replied to it; empty before.
    pub title: String,
    /// What the person called it; empty when never named.
    pub name: String,
    pub pinned: bool,
    pub archived: bool,
}

const CONVERSATION: &str =
    "SELECT id, bot, created_at, touched_at, title, name, pinned, archived FROM conversations";

fn conversation_row(r: &rusqlite::Row) -> Result<Conversation> {
    Ok(Conversation {
        id: r.get(0)?,
        bot: r.get(1)?,
        created_at: r.get(2)?,
        touched_at: r.get(3)?,
        title: r.get(4)?,
        name: r.get(5)?,
        pinned: r.get(6)?,
        archived: r.get(7)?,
    })
}

/// What checks a bot's answers (see Check before done in PROTOCOL.md).
#[derive(Clone, Debug, PartialEq)]
pub enum CheckWith {
    Off,
    /// The bot's own model.
    Own,
    /// A model by name, on the bot's own sign-in or key.
    Model(String),
    /// Another bot of the project, by id: its model and its instructions.
    Bot(String),
}

/// A prompt that waits for its bot to be free.
pub struct Queued {
    pub conversation: String,
    pub nonce: String,
    pub text: String,
}

/// A message from one bot to another, kept until it is answered and after.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Mail {
    pub seq: i64,
    pub from: String,
    pub to: String,
    pub text: String,
    /// `waiting` for the bot `to`, `working` with it, or `answered`.
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub answer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub at_ms: u64,
    /// The message its sender was answering when it sent this one (see `mail_askers`).
    #[serde(skip)]
    pub parent: Option<i64>,
}

/// One line of a project's room: what the person or a bot wrote there.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoomEntry {
    pub seq: i64,
    /// The bot's id; empty for the person.
    pub from: String,
    /// The bot it was addressed to; empty for a bot's reply, which is for the whole room.
    pub to: String,
    pub text: String,
    pub at_ms: u64,
}

const ROOM: &str = "SELECT seq, author, to_bot, text, at FROM room";

fn room_row(r: &rusqlite::Row) -> Result<RoomEntry> {
    Ok(RoomEntry {
        seq: r.get(0)?,
        from: r.get(1)?,
        to: r.get(2)?,
        text: r.get(3)?,
        at_ms: r.get(4)?,
    })
}

const MAIL: &str = "SELECT seq, from_bot, to_bot, text, state, answer, error, at, parent FROM mail";

fn mail_row(r: &rusqlite::Row) -> Result<Mail> {
    Ok(Mail {
        seq: r.get(0)?,
        from: r.get(1)?,
        to: r.get(2)?,
        text: r.get(3)?,
        state: r.get(4)?,
        answer: r.get(5)?,
        error: r.get(6)?,
        at_ms: r.get(7)?,
        parent: r.get(8)?,
    })
}

/// An installed workplace: what was approved of it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Workplace {
    pub name: String,
    pub version: String,
    /// SHA-256 of its files, as hex (see `workplaces::fingerprint`).
    pub fingerprint: String,
    /// SHA-256 of the catalog's archive it was installed from: the version it is pinned to.
    pub archive: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Profile {
    pub instructions: String,
    pub avatar: String,
    pub color: String,
}

/// A prompt's place in the ledger. A nonce is sent to the engine at most once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ledger {
    /// Written before the prompt goes to the engine; not yet admitted.
    Sending,
    /// The engine admitted it.
    Sent,
    /// The engine or the service stopped while it was `Sending`: the engine
    /// may or may not have it.
    Uncertain,
}

impl Ledger {
    fn as_str(self) -> &'static str {
        match self {
            Ledger::Sending => "sending",
            Ledger::Sent => "sent",
            Ledger::Uncertain => "uncertain",
        }
    }
    fn parse(s: &str) -> Ledger {
        match s {
            "sent" => Ledger::Sent,
            "sending" => Ledger::Sending,
            _ => Ledger::Uncertain,
        }
    }
}

/// Every relation is a foreign key: the rows of a bot go with it (`ON DELETE CASCADE`), and a
/// bot's project and a project's lead are cleared when what they point at is deleted.
const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS bots(id TEXT PRIMARY KEY, name TEXT NOT NULL, created_at INTEGER NOT NULL, project TEXT REFERENCES projects(id) ON DELETE SET NULL);
CREATE INDEX IF NOT EXISTS bots_project ON bots(project);
CREATE TABLE IF NOT EXISTS conversations(id TEXT PRIMARY KEY, bot TEXT NOT NULL REFERENCES bots(id) ON DELETE CASCADE, created_at INTEGER NOT NULL, touched_at INTEGER NOT NULL, session TEXT, title TEXT NOT NULL DEFAULT '', name TEXT NOT NULL DEFAULT '', pinned INTEGER NOT NULL DEFAULT 0, archived INTEGER NOT NULL DEFAULT 0);
CREATE INDEX IF NOT EXISTS conversations_bot ON conversations(bot, touched_at);
CREATE TABLE IF NOT EXISTS transcript(seq INTEGER PRIMARY KEY AUTOINCREMENT, conversation TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE, at INTEGER NOT NULL, entry TEXT NOT NULL);
CREATE INDEX IF NOT EXISTS transcript_conversation ON transcript(conversation, seq);
CREATE TABLE IF NOT EXISTS ledger(bot TEXT NOT NULL REFERENCES bots(id) ON DELETE CASCADE, nonce TEXT NOT NULL, state TEXT NOT NULL, at INTEGER NOT NULL, PRIMARY KEY(bot, nonce));
CREATE TABLE IF NOT EXISTS queue(seq INTEGER PRIMARY KEY, conversation TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE, nonce TEXT NOT NULL, text TEXT NOT NULL, UNIQUE(conversation, nonce));
CREATE TABLE IF NOT EXISTS questions(conversation TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE, question TEXT NOT NULL, entry TEXT NOT NULL, PRIMARY KEY(conversation, question));
CREATE TABLE IF NOT EXISTS access(bot TEXT PRIMARY KEY REFERENCES bots(id) ON DELETE CASCADE, whole_disk INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS network(bot TEXT PRIMARY KEY REFERENCES bots(id) ON DELETE CASCADE, hosts TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS agents(bot TEXT PRIMARY KEY REFERENCES bots(id) ON DELETE CASCADE, command TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS permissions(bot TEXT NOT NULL REFERENCES bots(id) ON DELETE CASCADE, kind TEXT NOT NULL, level TEXT NOT NULL, PRIMARY KEY(bot, kind));
CREATE TABLE IF NOT EXISTS grants(bot TEXT NOT NULL REFERENCES bots(id) ON DELETE CASCADE, kind TEXT NOT NULL, target TEXT NOT NULL, at INTEGER NOT NULL, PRIMARY KEY(bot, kind, target));
CREATE TABLE IF NOT EXISTS conversation_grants(conversation TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE, kind TEXT NOT NULL, target TEXT NOT NULL, at INTEGER NOT NULL, PRIMARY KEY(conversation, kind, target));
CREATE TABLE IF NOT EXISTS usage(bot TEXT NOT NULL REFERENCES bots(id) ON DELETE CASCADE, day TEXT NOT NULL, conversation TEXT NOT NULL, input INTEGER NOT NULL DEFAULT 0, cached INTEGER NOT NULL DEFAULT 0, output INTEGER NOT NULL DEFAULT 0, unreported INTEGER NOT NULL DEFAULT 0, PRIMARY KEY(bot, day, conversation));
CREATE TABLE IF NOT EXISTS token_limits(bot TEXT PRIMARY KEY REFERENCES bots(id) ON DELETE CASCADE, day INTEGER, month INTEGER, everything INTEGER NOT NULL DEFAULT 0);
CREATE TABLE IF NOT EXISTS blocked_tools(bot TEXT NOT NULL REFERENCES bots(id) ON DELETE CASCADE, tool TEXT NOT NULL, PRIMARY KEY(bot, tool));
CREATE TABLE IF NOT EXISTS routines(bot TEXT NOT NULL REFERENCES bots(id) ON DELETE CASCADE, id TEXT NOT NULL, routine TEXT NOT NULL, PRIMARY KEY(bot, id));
CREATE TABLE IF NOT EXISTS routine_runs(seq INTEGER PRIMARY KEY AUTOINCREMENT, bot TEXT NOT NULL REFERENCES bots(id) ON DELETE CASCADE, routine TEXT NOT NULL, run TEXT NOT NULL);
CREATE INDEX IF NOT EXISTS routine_runs_of ON routine_runs(bot, routine, seq);
CREATE TABLE IF NOT EXISTS oauth(bot TEXT NOT NULL REFERENCES bots(id) ON DELETE CASCADE, name TEXT NOT NULL, key TEXT NOT NULL UNIQUE, tokens TEXT, PRIMARY KEY(bot, name));
CREATE TABLE IF NOT EXISTS secrets(bot TEXT NOT NULL REFERENCES bots(id) ON DELETE CASCADE, name TEXT NOT NULL, site TEXT NOT NULL, PRIMARY KEY(bot, name));
CREATE TABLE IF NOT EXISTS projects(id TEXT PRIMARY KEY, project TEXT NOT NULL, lead TEXT REFERENCES bots(id) ON DELETE SET NULL);
CREATE TABLE IF NOT EXISTS workplaces(name TEXT PRIMARY KEY, version TEXT NOT NULL, fingerprint TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS workplace_archives(name TEXT PRIMARY KEY, sha256 TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS connectors(bot TEXT NOT NULL REFERENCES bots(id) ON DELETE CASCADE, name TEXT NOT NULL, connector TEXT NOT NULL, PRIMARY KEY(bot, name));
CREATE TABLE IF NOT EXISTS skills_off(bot TEXT NOT NULL REFERENCES bots(id) ON DELETE CASCADE, name TEXT NOT NULL, PRIMARY KEY(bot, name));
CREATE TABLE IF NOT EXISTS profiles(bot TEXT PRIMARY KEY REFERENCES bots(id) ON DELETE CASCADE, instructions TEXT NOT NULL DEFAULT '', avatar TEXT NOT NULL DEFAULT '', color TEXT NOT NULL DEFAULT '', position INTEGER);
CREATE TABLE IF NOT EXISTS models(bot TEXT PRIMARY KEY REFERENCES bots(id) ON DELETE CASCADE, model TEXT NOT NULL, effort TEXT);
CREATE TABLE IF NOT EXISTS step_limits(bot TEXT PRIMARY KEY REFERENCES bots(id) ON DELETE CASCADE, steps INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS checks(bot TEXT PRIMARY KEY REFERENCES bots(id) ON DELETE CASCADE, kind TEXT NOT NULL, with_what TEXT NOT NULL DEFAULT '');
CREATE TABLE IF NOT EXISTS paused(bot TEXT PRIMARY KEY REFERENCES bots(id) ON DELETE CASCADE);
CREATE TABLE IF NOT EXISTS web_search(bot TEXT PRIMARY KEY REFERENCES bots(id) ON DELETE CASCADE, enabled INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS providers(id INTEGER PRIMARY KEY CHECK (id = 1), url TEXT NOT NULL, key TEXT);
CREATE TABLE IF NOT EXISTS quick_ask(id INTEGER PRIMARY KEY CHECK (id = 1), shortcut TEXT NOT NULL, selection INTEGER NOT NULL, screenshot INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS chatgpt(id INTEGER PRIMARY KEY CHECK (id = 1), host_id TEXT NOT NULL, client_id TEXT NOT NULL, access_token TEXT NOT NULL, refresh_token TEXT NOT NULL, expires_ms INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS channels(bot TEXT NOT NULL REFERENCES bots(id) ON DELETE CASCADE, kind TEXT NOT NULL, token TEXT NOT NULL, PRIMARY KEY(bot, kind));
CREATE TABLE IF NOT EXISTS chats(bot TEXT NOT NULL REFERENCES bots(id) ON DELETE CASCADE, kind TEXT NOT NULL, chat TEXT NOT NULL, name TEXT NOT NULL, PRIMARY KEY(bot, kind, chat));
CREATE TABLE IF NOT EXISTS mail(seq INTEGER PRIMARY KEY AUTOINCREMENT, from_bot TEXT NOT NULL REFERENCES bots(id) ON DELETE CASCADE, to_bot TEXT NOT NULL REFERENCES bots(id) ON DELETE CASCADE, text TEXT NOT NULL, state TEXT NOT NULL, answer TEXT, error TEXT, at INTEGER NOT NULL, parent INTEGER);
CREATE TABLE IF NOT EXISTS room(seq INTEGER PRIMARY KEY AUTOINCREMENT, project TEXT NOT NULL, author TEXT NOT NULL, to_bot TEXT NOT NULL, text TEXT NOT NULL, at INTEGER NOT NULL);
CREATE INDEX IF NOT EXISTS room_project ON room(project, seq);
CREATE INDEX IF NOT EXISTS mail_to ON mail(to_bot, state, seq);
CREATE INDEX IF NOT EXISTS mail_from ON mail(from_bot);
CREATE TABLE IF NOT EXISTS file_copies(n INTEGER PRIMARY KEY AUTOINCREMENT, id TEXT NOT NULL UNIQUE, bot TEXT REFERENCES bots(id) ON DELETE CASCADE, project TEXT REFERENCES projects(id) ON DELETE CASCADE, conversation TEXT NOT NULL, path TEXT NOT NULL, earlier BLOB, after_sum TEXT, who TEXT NOT NULL, at INTEGER NOT NULL, undone_at INTEGER, told INTEGER NOT NULL DEFAULT 0);
CREATE INDEX IF NOT EXISTS file_copies_path ON file_copies(path);
CREATE TABLE IF NOT EXISTS seen(conversation TEXT PRIMARY KEY REFERENCES conversations(id) ON DELETE CASCADE, seq INTEGER NOT NULL);
";

/// What `PRAGMA user_version` holds: 0 before relations were foreign keys, 1 for those, 2 once
/// a bot has many conversations and the transcript, the waiting prompts and the open questions
/// belong to a conversation.
const SCHEMA_VERSION: i64 = 2;

/// Every table that holds rows of a bot, in its `bot` column, besides `bots`, `mail` and
/// `conversations`.
const BOT_TABLES: [&str; 23] = [
    "ledger",
    "access",
    "network",
    "agents",
    "permissions",
    "grants",
    "blocked_tools",
    "routines",
    "routine_runs",
    "oauth",
    "connectors",
    "skills_off",
    "profiles",
    "models",
    "step_limits",
    "web_search",
    "checks",
    "paused",
    "channels",
    "chats",
    "usage",
    "token_limits",
    "secrets",
];

/// The tables that hold rows of a conversation, in their `conversation` column. A database of
/// version 1 or less holds them in a `bot` column; its bot's one conversation takes them.
const CONVERSATION_TABLES: [&str; 3] = ["transcript", "queue", "questions"];

/// An error of the kind the database gives for a row it refuses.
fn refused(why: &str) -> rusqlite::Error {
    let code = rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CONSTRAINT);
    rusqlite::Error::SqliteFailure(code, Some(why.to_owned()))
}

/// The `user_version` of `conn` when it holds a database from before this schema, which is to
/// be moved.
fn old_version(conn: &Connection) -> Result<Option<i64>> {
    let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    Ok((version < SCHEMA_VERSION && has_table(conn, "bots")?).then_some(version))
}

/// The move cannot be undone, and an older version cannot read its result: before it, keeps one
/// copy of the database next to it, `<file>.before-N`, readable by its owner only, where N is
/// one more than the version the database has (`before-1` holds one from before foreign keys,
/// `before-2` one of version 1). A copy that is already there is kept (a move that failed is
/// tried again from the same database). Without the copy there is no move.
fn back_up(conn: &Connection, path: &Path) -> Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    let Some(version) = old_version(conn)? else {
        return Ok(());
    };
    let file = path.file_name().expect("a database file").to_string_lossy();
    let (copy, part) = (
        path.with_file_name(format!("{file}.before-{}", version + 1)),
        path.with_file_name(format!("{file}.before-{}.part", version + 1)),
    );
    if copy.exists() {
        return Ok(());
    }
    let failed = |e: String| rusqlite::Error::ToSqlConversionFailure(e.into());
    let _ = std::fs::remove_file(&part);
    // `VACUUM INTO` writes into an empty file, so it can be made private first. Under another
    // name until whole: a half-written copy is not kept.
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&part)
        .map_err(|e| failed(format!("{}: {e}", part.display())))?;
    let target = part
        .to_str()
        .ok_or_else(|| failed("the path is not text".into()))?;
    let copied = conn
        .execute("VACUUM INTO ?1", [target])
        .map(drop)
        .and_then(|()| {
            std::fs::rename(&part, &copy).map_err(|e| failed(format!("{}: {e}", copy.display())))
        });
    if let Err(e) = copied {
        let _ = std::fs::remove_file(&part);
        return Err(e);
    }
    log!(
        "{}: kept as it was before the move to version {SCHEMA_VERSION}",
        copy.display()
    );
    Ok(())
}

/// Creates the tables, and first moves an `app.db` of an older version, once, in one
/// transaction. SQLite cannot add a foreign key to a table, so each table that gets one is
/// rebuilt, with foreign keys off as its documentation describes. A row that points at a bot
/// that does not exist is dropped, and logged. Each bot's transcript, waiting prompts, open
/// questions and engine session become those of its first conversation, which has the bot's id.
fn migrate(conn: &Connection) -> Result<()> {
    conn.pragma_update(None, "foreign_keys", false)?;
    let tx = conn.unchecked_transaction()?;
    let old = old_version(&tx)?;
    // The tables to rebuild: an older database may lack some that a still older one never made.
    let mut moved = Vec::new();
    let all = BOT_TABLES.into_iter().chain(CONVERSATION_TABLES);
    for table in all.chain(["mail", "projects", "bots"]) {
        if old.is_some() && has_table(&tx, table)? {
            tx.execute(&format!("ALTER TABLE {table} RENAME TO {table}_old"), [])?;
            moved.push(table);
        }
    }
    if old.is_some() {
        // The indexes stayed with their tables; the new ones take their names.
        let mut stmt = tx.prepare(
            "SELECT name FROM sqlite_master WHERE type = 'index' AND sql IS NOT NULL AND tbl_name LIKE '%\\_old' ESCAPE '\\'",
        )?;
        let indexes: Vec<String> = stmt.query_map([], |r| r.get(0))?.collect::<Result<_>>()?;
        for index in indexes {
            tx.execute(&format!("DROP INDEX {index}"), [])?;
        }
    }
    let first_seen = !has_table(&tx, "seen")?;
    tx.execute_batch(SCHEMA)?;
    // A database of version 2 made before a message kept the one its sender was working on.
    if !has_column(&tx, "mail", "parent")? {
        tx.execute("ALTER TABLE mail ADD COLUMN parent INTEGER", [])?;
    }
    if let Some(version) = old {
        copy_old(&tx, "bots", "1")?;
        let known = "IN (SELECT id FROM bots)";
        for table in BOT_TABLES {
            copy_old(&tx, table, &format!("bot {known}"))?;
        }
        copy_old(&tx, "mail", &format!("from_bot {known} AND to_bot {known}"))?;
        if version == 0 {
            copy_old_projects(&tx)?;
        } else {
            copy_old(&tx, "projects", "1")?;
        }
        // One conversation for each bot, which has the engine's session the bot had.
        let session = if has_column(&tx, "bots_old", "session")? {
            "o.session"
        } else {
            "NULL"
        };
        tx.execute(
            &format!(
                "INSERT INTO conversations(id, bot, created_at, touched_at, session) \
                 SELECT b.id, b.id, b.created_at, b.created_at, {session} FROM bots b JOIN bots_old o ON o.id = b.id"
            ),
            [],
        )?;
        for table in CONVERSATION_TABLES {
            copy_old_conversation_rows(&tx, table)?;
        }
        tx.execute(
            "UPDATE conversations SET touched_at = COALESCE((SELECT max(at) FROM transcript WHERE conversation = conversations.id), created_at)",
            [],
        )?;
        let mut stmt = tx.prepare("SELECT id FROM conversations")?;
        let ids: Vec<String> = stmt.query_map([], |r| r.get(0))?.collect::<Result<_>>()?;
        for id in ids {
            give_title(&tx, &id)?;
        }
        for table in moved {
            // A table's last number is not given again.
            tx.execute("DELETE FROM sqlite_sequence WHERE name = ?1", [table])?;
            tx.execute(
                "UPDATE sqlite_sequence SET name = ?1 WHERE name = ?1 || '_old'",
                [table],
            )?;
            tx.execute(&format!("DROP TABLE {table}_old"), [])?;
        }
        let broken: i64 =
            tx.query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| {
                r.get(0)
            })?;
        if broken > 0 {
            return Err(refused("the moved rows break a foreign key"));
        }
    }
    if first_seen {
        // What was there before the person could look is not news.
        tx.execute(
            "INSERT INTO seen(conversation, seq) SELECT conversation, max(seq) FROM transcript GROUP BY conversation",
            [],
        )?;
    }
    tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    tx.commit()
}

fn has_table(conn: &Connection, name: &str) -> Result<bool> {
    conn.prepare("SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1")?
        .exists([name])
}

fn has_column(conn: &Connection, table: &str, column: &str) -> Result<bool> {
    conn.prepare("SELECT 1 FROM pragma_table_info(?1) WHERE name = ?2")?
        .exists([table, column])
}

/// Copies the rows of `{table}_old`, which hold a `bot`, into `table`, which holds a
/// `conversation` instead: the one conversation each bot has, which has the bot's id.
fn copy_old_conversation_rows(tx: &Connection, table: &str) -> Result<()> {
    if !has_table(tx, &format!("{table}_old"))? {
        return Ok(());
    }
    let (columns, from): (String, String) = tx.query_row(
        &format!(
            "SELECT group_concat('\"' || n.name || '\"'), group_concat('o.\"' || n.name || '\"') \
             FROM pragma_table_info('{table}') n JOIN pragma_table_info('{table}_old') o ON o.name = n.name"
        ),
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    tx.execute(
        &format!(
            "INSERT INTO {table}(conversation, {columns}) \
             SELECT c.id, {from} FROM {table}_old o JOIN conversations c ON c.bot = o.bot"
        ),
        [],
    )?;
    let dropped: i64 = tx.query_row(
        &format!("SELECT (SELECT count(*) FROM {table}_old) - (SELECT count(*) FROM {table})"),
        [],
        |r| r.get(0),
    )?;
    if dropped > 0 {
        log!("app.db: {dropped} rows of {table} pointed at a bot that does not exist; dropped");
    }
    Ok(())
}

/// Longest title, in characters.
const TITLE_CHARS: usize = 60;

/// A title for a conversation made from its first prompt: its first line with the spaces in it
/// squeezed, cut short.
fn title_of(prompt: &str) -> String {
    let line = prompt.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    let line = line.split_whitespace().collect::<Vec<_>>().join(" ");
    match line.char_indices().nth(TITLE_CHARS) {
        Some((end, _)) => format!("{}…", &line[..end]),
        None => line,
    }
}

/// Gives a conversation that has no title one, from its first prompt, if it has one. `true`
/// when it got one.
fn give_title(conn: &Connection, conversation: &str) -> Result<bool> {
    let first: Option<String> = conn
        .query_row(
            "SELECT json_extract(entry, '$.text') FROM transcript WHERE conversation = ?1 \
             AND json_extract(entry, '$.type') = 'prompt' ORDER BY seq LIMIT 1",
            [conversation],
            |r| r.get(0),
        )
        .optional()?
        .flatten();
    let title = first.as_deref().map(title_of).unwrap_or_default();
    Ok(conn.execute(
        "UPDATE conversations SET title = ?2 WHERE id = ?1 AND title = '' AND ?2 != ''",
        params![conversation, title],
    )? > 0)
}

/// Copies the rows of `{table}_old` that satisfy `keep` into `table`, in the columns both have.
fn copy_old(tx: &Connection, table: &str, keep: &str) -> Result<()> {
    if !has_table(tx, &format!("{table}_old"))? {
        return Ok(());
    }
    let columns: String = tx.query_row(
        &format!(
            "SELECT group_concat('\"' || n.name || '\"') FROM pragma_table_info('{table}') n \
             JOIN pragma_table_info('{table}_old') o ON o.name = n.name"
        ),
        [],
        |r| r.get(0),
    )?;
    tx.execute(
        &format!("INSERT INTO {table}({columns}) SELECT {columns} FROM {table}_old WHERE {keep}"),
        [],
    )?;
    let dropped: i64 = tx.query_row(
        &format!("SELECT (SELECT count(*) FROM {table}_old) - (SELECT count(*) FROM {table})"),
        [],
        |r| r.get(0),
    )?;
    if dropped > 0 {
        log!("app.db: {dropped} rows of {table} pointed at a bot that does not exist; dropped");
    }
    Ok(())
}

/// Moves the projects: the bots and the lead, which were inside each one's JSON, become the
/// bots' `project` and the project's `lead`. A bot that is gone, or already in another project,
/// is left out; a lead that is not one of the project's bots is left out.
fn copy_old_projects(tx: &Connection) -> Result<()> {
    if !has_table(tx, "projects_old")? {
        return Ok(());
    }
    let mut stmt = tx.prepare("SELECT id, project FROM projects_old ORDER BY id")?;
    let old: Vec<(String, String)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_>>()?;
    for (id, text) in old {
        let mut project: Value = parse(text)?;
        let mut take = |key: &str| project.as_object_mut().and_then(|o| o.remove(key));
        let bots: Vec<String> = take("bots")
            .and_then(|b| serde_json::from_value(b).ok())
            .unwrap_or_default();
        let lead = take("lead").and_then(|l| l.as_str().map(str::to_owned));
        tx.execute(
            "INSERT INTO projects(id, project) VALUES (?1, ?2)",
            params![id, json(&project)],
        )?;
        for bot in bots {
            let put = tx.execute(
                "UPDATE bots SET project = ?1 WHERE id = ?2 AND project IS NULL",
                [&id, &bot],
            )?;
            if put == 0 {
                log!(
                    "app.db: bot {bot} left out of project {id}: it is gone or in another project"
                );
            }
        }
        tx.execute(
            "UPDATE projects SET lead = ?2 WHERE id = ?1 AND ?2 IN (SELECT id FROM bots WHERE project = ?1)",
            params![id, lead],
        )?;
    }
    Ok(())
}

/// Past runs kept per routine.
const MAX_RUNS: u32 = 50;

/// A place `search` found the words.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Hit {
    pub bot: String,
    pub conversation: String,
    pub seq: i64,
    /// When the words were written, in Unix milliseconds.
    pub at_ms: u64,
    /// `user` (a prompt) or `reply`.
    pub kind: &'static str,
    pub snippet: String,
}

/// Up to 40 characters either side of the first `needle` (lower case) in `text`, if there is one.
fn snippet(text: &str, needle: &str) -> Option<String> {
    // Lower case can change lengths ('İ' is two characters): each lower-case byte keeps the
    // place of the character it came from.
    let chars: Vec<char> = text.chars().collect();
    let mut lower = String::new();
    let mut from_char = Vec::new();
    for (i, c) in chars.iter().enumerate() {
        for l in c.to_lowercase() {
            lower.push(l);
            from_char.resize(lower.len(), i);
        }
    }
    let at = lower.find(needle)?;
    let (start, end) = (from_char[at], from_char[at + needle.len() - 1] + 1);
    let (from, to) = (start.saturating_sub(40), (end + 40).min(chars.len()));
    let mut shown: String = chars[from..to].iter().collect();
    if from > 0 {
        shown.insert(0, '…');
    }
    if to < chars.len() {
        shown.push('…');
    }
    Some(shown.split_whitespace().collect::<Vec<_>>().join(" "))
}

/// What a bot may use, in tokens; a missing number is no limit. `everything`: the person's own
/// work is stopped too, not only unattended work.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TokenLimit {
    pub day: Option<u64>,
    pub month: Option<u64>,
    pub everything: bool,
}

fn json<T: Serialize>(v: &T) -> String {
    serde_json::to_string(v).expect("serializable")
}

fn parse<T: serde::de::DeserializeOwned>(text: String) -> rusqlite::Result<T> {
    serde_json::from_str(&text).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
    })
}

/// Where the Keychain keeps each secret; every account is under the service in `keychain`.
const PROVIDER_KEY: &str = "provider";
const CHATGPT_ACCESS: &str = "chatgpt.access";
const CHATGPT_REFRESH: &str = "chatgpt.refresh";

fn oauth_account(bot: &str, name: &str) -> String {
    format!("oauth:{bot}:{name}")
}

fn channel_account(bot: &str, kind: &str) -> String {
    format!("channel:{bot}:{kind}")
}

fn secret_account(bot: &str, name: &str) -> String {
    format!("secret:{bot}:{name}")
}

fn kc<T>(r: keychain::Result<T>) -> Result<T> {
    // No error variant fits better: this one only carries the keychain's error on, for
    // `core::internal` to find.
    r.map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))
}

/// Keeps `secret` under `account` (an empty one forgets it), and gives what must go in its
/// column of `app.db` in the meantime: nothing when the Keychain has it, the secret itself when
/// the Keychain refuses it (a prompt denied, a locked keychain). Dropping it then could lose a
/// token the server has already replaced; `move_secrets` tries again at the next start.
// shortcut: forgetting a secret the Keychain refuses to delete only logs, so an item that
// outlives a sign-out comes back when the Keychain works again; upgrade with a tombstone if
// that is ever seen (the sign-out is also revoked at the server).
fn keep(account: &str, secret: &str) -> Result<Option<String>> {
    match keychain::put(account, secret) {
        Ok(()) => Ok(None),
        // Only a removal waits (see `keychain::put`): it is not done while it waits, else the
        // item would stay and be read again as if it had never been signed out.
        Err(e @ (keychain::Error::Waiting | keychain::Error::Refused)) => kc(Err(e)),
        Err(e) => {
            log!("{account} stays in app.db: the Keychain did not keep it: {e}");
            Ok(Some(secret.to_owned()).filter(|s| !s.is_empty()))
        }
    }
}

/// The secret kept under `account`. `legacy` is what `app.db` holds in its place: a secret not
/// moved yet, or one the Keychain refused since. It is the newer: a refused one is written
/// while the Keychain may still hold an older value.
fn secret(account: &str, legacy: String) -> Result<String> {
    if !legacy.is_empty() {
        return Ok(legacy);
    }
    match keychain::get(account) {
        Ok(found) => Ok(found.unwrap_or_default()),
        Err(e) => kc(Err(e)),
    }
}

/// Moves the secrets an older `app.db` holds into the Keychain. Each is written, read back, and
/// only then blanked in the database: a secret the Keychain refuses stays where it was (and in
/// use, see `secret`) and is tried again at the next start.
fn move_secrets(db: &Connection) -> Result<()> {
    // (account, secret, statement that blanks it, its parameters)
    let mut found: Vec<(String, String, &str, Vec<String>)> = Vec::new();
    let key: Option<String> = db
        .query_row("SELECT key FROM providers WHERE id = 1", [], |r| r.get(0))
        .optional()?
        .flatten();
    if let Some(key) = key {
        let blank = "UPDATE providers SET key = NULL";
        found.push((PROVIDER_KEY.into(), key, blank, vec![]));
    }
    let tokens = db
        .query_row(
            "SELECT access_token, refresh_token FROM chatgpt WHERE id = 1",
            [],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
        )
        .optional()?;
    if let Some((access, refresh)) = tokens {
        let blank = "UPDATE chatgpt SET access_token = ''";
        found.push((CHATGPT_ACCESS.into(), access, blank, vec![]));
        let blank = "UPDATE chatgpt SET refresh_token = ''";
        found.push((CHATGPT_REFRESH.into(), refresh, blank, vec![]));
    }
    let mut stmt = db.prepare("SELECT bot, name, tokens FROM oauth WHERE tokens IS NOT NULL")?;
    for row in stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get::<_, String>(2)?)))? {
        let (bot, name, text): (String, String, String) = row?;
        let blank = "UPDATE oauth SET tokens = NULL WHERE bot = ?1 AND name = ?2";
        found.push((oauth_account(&bot, &name), text, blank, vec![bot, name]));
    }
    let mut stmt = db.prepare("SELECT bot, kind, token FROM channels")?;
    for row in stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))? {
        let (bot, kind, token): (String, String, String) = row?;
        let blank = "UPDATE channels SET token = '' WHERE bot = ?1 AND kind = ?2";
        found.push((channel_account(&bot, &kind), token, blank, vec![bot, kind]));
    }
    found.retain(|(_, secret, ..)| !secret.is_empty());
    let moved = !found.is_empty();
    for (account, secret, blank, params) in found {
        let kept = keychain::put(&account, &secret).and_then(|()| keychain::get(&account));
        if !matches!(kept, Ok(Some(back)) if back == secret) {
            log!("{account} stays in app.db: the Keychain did not keep it");
            continue;
        }
        db.execute(blank, rusqlite::params_from_iter(params))?;
    }
    if moved {
        // The old pages must not linger in the write-ahead log.
        db.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))?;
    }
    Ok(())
}

impl Store {
    /// Opens (or creates) the database. Engines do not outlive the service, so
    /// a prompt still `sending` becomes uncertain and open questions are dropped.
    pub fn open(path: &Path) -> Result<Store> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        // FULL: a committed ledger row survives power loss, not only a crash.
        conn.pragma_update(None, "synchronous", "FULL")?;
        // A blanked secret must not stay in the file as freed bytes.
        conn.pragma_update(None, "secure_delete", true)?;
        back_up(&conn, path)?;
        migrate(&conn)?;
        // Off in SQLite unless asked, for each connection.
        conn.pragma_update(None, "foreign_keys", true)?;
        // Groups are gone (ADR 0002): a project is how bots work together.
        conn.execute("DROP TABLE IF EXISTS groups", [])?;
        conn.execute(
            "UPDATE ledger SET state = 'uncertain' WHERE state = 'sending'",
            [],
        )?;
        conn.execute("DELETE FROM questions", [])?;
        move_secrets(&conn)?;
        Ok(Store(conn))
    }

    /// A new bot has one conversation.
    pub fn create_bot(&self, id: &str, name: &str, now: u64) -> Result<BotRow> {
        self.create_bot_with(id, name, now, None)
    }

    /// A bot made with a profile, routines and connectors too, all or nothing (a bot file).
    pub fn create_bot_with(
        &self,
        id: &str,
        name: &str,
        now: u64,
        rest: Option<(&Profile, &[Routine], &[Connector])>,
    ) -> Result<BotRow> {
        let tx = self.0.unchecked_transaction()?;
        tx.execute(
            "INSERT INTO bots(id, name, created_at) VALUES (?1, ?2, ?3)",
            params![id, name, now],
        )?;
        tx.execute(
            "INSERT INTO conversations(id, bot, created_at, touched_at) VALUES (?1, ?2, ?3, ?3)",
            params![random_hex(4), id, now],
        )?;
        // The same connection: these run inside the transaction, and a failure rolls back all.
        if let Some((profile, routines, connectors)) = rest {
            self.set_profile(id, profile)?;
            for r in routines {
                self.put_routine(id, r)?;
            }
            for c in connectors {
                self.put_connector(id, c)?;
            }
        }
        tx.commit()?;
        Ok(BotRow {
            id: id.to_owned(),
            name: name.to_owned(),
            created_at: now,
        })
    }

    pub fn bots(&self) -> Result<Vec<BotRow>> {
        let mut stmt = self.0.prepare_cached(
            "SELECT b.id, b.name, b.created_at FROM bots b LEFT JOIN profiles p ON p.bot = b.id \
                 ORDER BY p.position IS NULL, p.position, b.created_at, b.id",
        )?;
        stmt.query_map([], |r| {
            Ok(BotRow {
                id: r.get(0)?,
                name: r.get(1)?,
                created_at: r.get(2)?,
            })
        })?
        .collect()
    }

    pub fn rename_bot(&self, bot: &str, name: &str) -> Result<()> {
        self.0
            .execute("UPDATE bots SET name = ?2 WHERE id = ?1", [bot, name])
            .map(drop)
    }

    /// The bot's instructions, avatar and colour; empty when never set.
    pub fn profile(&self, bot: &str) -> Result<Profile> {
        self.0
            .query_row(
                "SELECT instructions, avatar, color FROM profiles WHERE bot = ?1",
                [bot],
                |r| {
                    Ok(Profile {
                        instructions: r.get(0)?,
                        avatar: r.get(1)?,
                        color: r.get(2)?,
                    })
                },
            )
            .optional()
            .map(Option::unwrap_or_default)
    }

    /// The model and thinking effort chosen for `bot`; `None`: the engine's default.
    pub fn model(&self, bot: &str) -> Result<Option<(String, Option<String>)>> {
        self.0
            .query_row(
                "SELECT model, effort FROM models WHERE bot = ?1",
                [bot],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
    }

    /// An empty `model`: back to the engine's default.
    pub fn set_model(&self, bot: &str, model: &str, effort: Option<&str>) -> Result<()> {
        if model.is_empty() {
            return self
                .0
                .execute("DELETE FROM models WHERE bot = ?1", [bot])
                .map(drop);
        }
        self.0
            .execute(
                "INSERT INTO models(bot, model, effort) VALUES (?1, ?2, ?3) \
                 ON CONFLICT(bot) DO UPDATE SET model = ?2, effort = ?3",
                params![bot, model, effort],
            )
            .map(drop)
    }

    /// The tool rounds a turn of `bot` may take; `None`: the default.
    pub fn step_limit(&self, bot: &str) -> Result<Option<u32>> {
        self.0
            .query_row("SELECT steps FROM step_limits WHERE bot = ?1", [bot], |r| {
                r.get(0)
            })
            .optional()
    }

    pub fn set_step_limit(&self, bot: &str, steps: u32) -> Result<()> {
        self.0
            .execute(
                "INSERT INTO step_limits(bot, steps) VALUES (?1, ?2) \
                 ON CONFLICT(bot) DO UPDATE SET steps = ?2",
                params![bot, steps],
            )
            .map(drop)
    }

    /// Whether the provider's web search tool is offered to `bot`; on unless turned off.
    pub fn web_search(&self, bot: &str) -> Result<bool> {
        let row = self
            .0
            .query_row(
                "SELECT enabled FROM web_search WHERE bot = ?1",
                [bot],
                |r| r.get(0),
            )
            .optional()?;
        Ok(row.unwrap_or(true))
    }

    pub fn set_web_search(&self, bot: &str, enabled: bool) -> Result<()> {
        self.0
            .execute(
                "INSERT INTO web_search(bot, enabled) VALUES (?1, ?2) \
                 ON CONFLICT(bot) DO UPDATE SET enabled = ?2",
                params![bot, enabled],
            )
            .map(drop)
    }

    /// What checks the bot's answers; off unless chosen.
    pub fn check(&self, bot: &str) -> Result<CheckWith> {
        let row: Option<(String, String)> = self
            .0
            .query_row(
                "SELECT kind, with_what FROM checks WHERE bot = ?1",
                [bot],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        Ok(match row {
            Some((kind, with)) => match kind.as_str() {
                "own" => CheckWith::Own,
                "model" => CheckWith::Model(with),
                "bot" => CheckWith::Bot(with),
                _ => CheckWith::Off,
            },
            None => CheckWith::Off,
        })
    }

    pub fn set_check(&self, bot: &str, with: &CheckWith) -> Result<()> {
        let (kind, what) = match with {
            CheckWith::Off => ("off", ""),
            CheckWith::Own => ("own", ""),
            CheckWith::Model(m) => ("model", m.as_str()),
            CheckWith::Bot(b) => ("bot", b.as_str()),
        };
        self.0
            .execute(
                "INSERT INTO checks(bot, kind, with_what) VALUES (?1, ?2, ?3) \
                 ON CONFLICT(bot) DO UPDATE SET kind = ?2, with_what = ?3",
                params![bot, kind, what],
            )
            .map(drop)
    }

    /// Whether the bot is paused (see Pause in PROTOCOL.md).
    pub fn paused(&self, bot: &str) -> Result<bool> {
        self.0
            .query_row("SELECT 1 FROM paused WHERE bot = ?1", [bot], |_| Ok(()))
            .optional()
            .map(|row| row.is_some())
    }

    pub fn set_paused(&self, bot: &str, paused: bool) -> Result<()> {
        let sql = match paused {
            true => "INSERT OR IGNORE INTO paused(bot) VALUES (?1)",
            false => "DELETE FROM paused WHERE bot = ?1",
        };
        self.0.execute(sql, [bot]).map(drop)
    }

    /// Adds one answered model request to what `bot` used on `day` (`YYYY-MM-DD`, local) in
    /// `conversation`. A request the server reported nothing for adds to `unreported`.
    pub fn add_usage(
        &self,
        bot: &str,
        day: &str,
        conversation: &str,
        reported: bool,
        used: proto::Tokens,
    ) -> Result<()> {
        self.0
            .execute(
                "INSERT INTO usage(bot, day, conversation, input, cached, output, unreported) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) \
                 ON CONFLICT(bot, day, conversation) DO UPDATE SET input = input + ?4, \
                 cached = cached + ?5, output = output + ?6, unreported = unreported + ?7",
                params![
                    bot,
                    day,
                    conversation,
                    used.input,
                    used.cached,
                    used.output,
                    u64::from(!reported)
                ],
            )
            .map(drop)
    }

    /// What `bot` used from `first` to `last` (days, inclusive), and how many requests of those
    /// days the server reported nothing for. Without `conversation`, in all conversations.
    pub fn usage(
        &self,
        bot: &str,
        first: &str,
        last: &str,
        conversation: Option<&str>,
    ) -> Result<(proto::Tokens, u64)> {
        self.0.query_row(
            "SELECT COALESCE(SUM(input), 0), COALESCE(SUM(cached), 0), COALESCE(SUM(output), 0), \
             COALESCE(SUM(unreported), 0) FROM usage \
             WHERE bot = ?1 AND day >= ?2 AND day <= ?3 AND (?4 IS NULL OR conversation = ?4)",
            params![bot, first, last, conversation],
            |r| {
                let used = proto::Tokens {
                    input: r.get(0)?,
                    cached: r.get(1)?,
                    output: r.get(2)?,
                };
                Ok((used, r.get(3)?))
            },
        )
    }

    /// The token limit of `bot`; none unless set.
    pub fn token_limit(&self, bot: &str) -> Result<TokenLimit> {
        let row = self
            .0
            .query_row(
                "SELECT day, month, everything FROM token_limits WHERE bot = ?1",
                [bot],
                |r| {
                    Ok(TokenLimit {
                        day: r.get(0)?,
                        month: r.get(1)?,
                        everything: r.get(2)?,
                    })
                },
            )
            .optional()?;
        Ok(row.unwrap_or_default())
    }

    pub fn set_token_limit(&self, bot: &str, limit: &TokenLimit) -> Result<()> {
        self.0
            .execute(
                "INSERT INTO token_limits(bot, day, month, everything) VALUES (?1, ?2, ?3, ?4) \
                 ON CONFLICT(bot) DO UPDATE SET day = ?2, month = ?3, everything = ?4",
                params![bot, limit.day, limit.month, limit.everything],
            )
            .map(drop)
    }

    /// The model server URL and optional API key configured in the service.
    pub fn provider(&self) -> Result<Option<(String, Option<String>)>> {
        let row: Option<(String, Option<String>)> = self
            .0
            .query_row("SELECT url, key FROM providers WHERE id = 1", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .optional()?;
        row.map(|(url, legacy)| {
            let key = secret(PROVIDER_KEY, legacy.unwrap_or_default())?;
            Ok((url, Some(key).filter(|k| !k.is_empty())))
        })
        .transpose()
    }

    pub fn set_provider(&self, url: &str, key: Option<&str>) -> Result<()> {
        let kept = keep(PROVIDER_KEY, key.unwrap_or_default())?;
        self.0
            .execute(
                "INSERT INTO providers (id, url, key) VALUES (1, ?1, ?2) ON CONFLICT(id) DO UPDATE SET url = ?1, key = ?2",
                params![url, kept],
            )
            .map(drop)
    }

    /// What the person set for Quick Ask; everything off until they do.
    pub fn quick_ask(&self) -> Result<proto::quick::Settings> {
        let row = self
            .0
            .query_row(
                "SELECT shortcut, selection, screenshot FROM quick_ask WHERE id = 1",
                [],
                |r| {
                    Ok(proto::quick::Settings {
                        shortcut: Some(r.get::<_, String>(0)?).filter(|s| !s.is_empty()),
                        selection: r.get(1)?,
                        screenshot: r.get(2)?,
                    })
                },
            )
            .optional()?;
        Ok(row.unwrap_or_default())
    }

    pub fn set_quick_ask(&self, quick: &proto::quick::Settings) -> Result<()> {
        self.0
            .execute(
                "INSERT INTO quick_ask (id, shortcut, selection, screenshot) VALUES (1, ?1, ?2, ?3) ON CONFLICT(id) DO UPDATE SET shortcut = ?1, selection = ?2, screenshot = ?3",
                params![
                    quick.shortcut.as_deref().unwrap_or_default(),
                    quick.selection,
                    quick.screenshot
                ],
            )
            .map(drop)
    }

    /// Every bot's channels, as `(bot, kind, token)`.
    pub fn channels(&self) -> Result<Vec<(String, String, String)>> {
        let mut stmt = self
            .0
            .prepare_cached("SELECT bot, kind, token FROM channels")?;
        let rows: Vec<(String, String, String)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<Result<_>>()?;
        rows.into_iter()
            .map(|(bot, kind, legacy)| {
                let token = secret(&channel_account(&bot, &kind), legacy)?;
                Ok((bot, kind, token))
            })
            .collect()
    }

    /// Every bot's channels, as `(bot, kind)`, without their tokens.
    pub fn channel_keys(&self) -> Result<Vec<(String, String)>> {
        let mut stmt = self.0.prepare_cached("SELECT bot, kind FROM channels")?;
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect()
    }

    pub fn channel(&self, bot: &str, kind: &str) -> Result<Option<String>> {
        let legacy: Option<String> = self
            .0
            .query_row(
                "SELECT token FROM channels WHERE bot = ?1 AND kind = ?2",
                [bot, kind],
                |r| r.get(0),
            )
            .optional()?;
        legacy
            .map(|legacy| secret(&channel_account(bot, kind), legacy))
            .transpose()
    }

    pub fn set_channel(&self, bot: &str, kind: &str, token: &str) -> Result<()> {
        let kept = keep(&channel_account(bot, kind), token)?;
        self.0
            .execute(
                "INSERT OR REPLACE INTO channels(bot, kind, token) VALUES (?1, ?2, ?3)",
                params![bot, kind, kept.unwrap_or_default()],
            )
            .map(drop)
    }

    /// Forgets the channel and the chats that were let in on it.
    pub fn delete_channel(&self, bot: &str, kind: &str) -> Result<()> {
        kc(keychain::delete(&channel_account(bot, kind)))?;
        for table in ["channels", "chats"] {
            self.0.execute(
                &format!("DELETE FROM {table} WHERE bot = ?1 AND kind = ?2"),
                [bot, kind],
            )?;
        }
        Ok(())
    }

    /// The chats let in on a channel, as `(chat, name)`.
    pub fn chats(&self, bot: &str, kind: &str) -> Result<Vec<(String, String)>> {
        let mut stmt = self.0.prepare_cached(
            "SELECT chat, name FROM chats WHERE bot = ?1 AND kind = ?2 ORDER BY rowid",
        )?;
        stmt.query_map([bot, kind], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect()
    }

    pub fn put_chat(&self, bot: &str, kind: &str, chat: &str, name: &str) -> Result<()> {
        self.0
            .execute(
                "INSERT OR REPLACE INTO chats(bot, kind, chat, name) VALUES (?1, ?2, ?3, ?4)",
                [bot, kind, chat, name],
            )
            .map(drop)
    }

    pub fn delete_chat(&self, bot: &str, kind: &str, chat: &str) -> Result<bool> {
        Ok(self.0.execute(
            "DELETE FROM chats WHERE bot = ?1 AND kind = ?2 AND chat = ?3",
            [bot, kind, chat],
        )? > 0)
    }

    /// The registration with ChatGPT and, while signed in, its tokens.
    pub fn chatgpt(&self) -> Result<Option<crate::chatgpt::Saved>> {
        let columns = "host_id, client_id, access_token, refresh_token, expires_ms";
        let row = self
            .0
            .query_row(
                &format!("SELECT {columns} FROM chatgpt WHERE id = 1"),
                [],
                |r| {
                    Ok(crate::chatgpt::Saved {
                        host_id: r.get(0)?,
                        client_id: r.get(1)?,
                        access_token: r.get(2)?,
                        refresh_token: r.get(3)?,
                        expires_ms: r.get::<_, i64>(4)?.max(0) as u64,
                    })
                },
            )
            .optional()?;
        row.map(|mut s| {
            s.access_token = secret(CHATGPT_ACCESS, s.access_token)?;
            s.refresh_token = secret(CHATGPT_REFRESH, s.refresh_token)?;
            Ok(s)
        })
        .transpose()
    }

    pub fn set_chatgpt(&self, s: &crate::chatgpt::Saved) -> Result<()> {
        let expires = i64::try_from(s.expires_ms).unwrap_or(i64::MAX);
        let access = keep(CHATGPT_ACCESS, &s.access_token)?;
        let refresh = keep(CHATGPT_REFRESH, &s.refresh_token)?;
        self.0
            .execute(
                "INSERT OR REPLACE INTO chatgpt (id, host_id, client_id, access_token, refresh_token, expires_ms) VALUES (1, ?1, ?2, ?3, ?4, ?5)",
                params![s.host_id, s.client_id, access.unwrap_or_default(), refresh.unwrap_or_default(), expires],
            )
            .map(drop)
    }

    pub fn delete_provider(&self) -> Result<()> {
        kc(keychain::delete(PROVIDER_KEY))?;
        self.0
            .execute("DELETE FROM providers WHERE id = 1", [])
            .map(drop)
    }

    pub fn set_profile(&self, bot: &str, p: &Profile) -> Result<()> {
        self.0
            .execute(
                "INSERT INTO profiles(bot, instructions, avatar, color) VALUES (?1, ?2, ?3, ?4) \
                 ON CONFLICT(bot) DO UPDATE SET instructions = ?2, avatar = ?3, color = ?4",
                params![bot, p.instructions, p.avatar, p.color],
            )
            .map(drop)
    }

    /// Puts the bots in this order.
    pub fn order_bots(&mut self, bots: &[String]) -> Result<()> {
        let tx = self.0.transaction()?;
        for (i, bot) in bots.iter().enumerate() {
            tx.execute(
                "INSERT INTO profiles(bot, position) VALUES (?1, ?2) \
                 ON CONFLICT(bot) DO UPDATE SET position = ?2",
                params![bot, i as i64],
            )?;
        }
        tx.commit()
    }

    /// Forgets the bot and everything stored for it, its secrets in the Keychain too. The
    /// database empties the tables of the bot and clears its place in a project, in this one
    /// statement.
    pub fn delete_bot(&self, bot: &str) -> Result<()> {
        for (table, column, account) in [
            (
                "channels",
                "kind",
                channel_account as fn(&str, &str) -> String,
            ),
            ("oauth", "name", oauth_account),
            ("secrets", "name", secret_account),
        ] {
            let mut stmt = self
                .0
                .prepare(&format!("SELECT {column} FROM {table} WHERE bot = ?1"))?;
            let names: Vec<String> = stmt
                .query_map([bot], |r| r.get(0))?
                .collect::<Result<_>>()?;
            for name in names {
                kc(keychain::delete(&account(bot, &name)))?;
            }
        }
        self.0
            .execute("DELETE FROM bots WHERE id = ?1", [bot])
            .map(drop)
    }

    pub fn bot(&self, id: &str) -> Result<Option<BotRow>> {
        self.0
            .query_row(
                "SELECT id, name, created_at FROM bots WHERE id = ?1",
                [id],
                |r| {
                    Ok(BotRow {
                        id: r.get(0)?,
                        name: r.get(1)?,
                        created_at: r.get(2)?,
                    })
                },
            )
            .optional()
    }

    /// The engine's session of a conversation; `None` until the engine made one.
    pub fn session(&self, conversation: &str) -> Result<Option<String>> {
        self.0.query_row(
            "SELECT session FROM conversations WHERE id = ?1",
            [conversation],
            |r| r.get(0),
        )
    }

    pub fn set_session(&self, conversation: &str, session: &str) -> Result<()> {
        self.0
            .execute(
                "UPDATE conversations SET session = ?2 WHERE id = ?1",
                params![conversation, session],
            )
            .map(drop)
    }

    pub fn clear_session(&self, conversation: &str) -> Result<()> {
        self.0
            .execute(
                "UPDATE conversations SET session = NULL WHERE id = ?1",
                [conversation],
            )
            .map(drop)
    }

    /// The bot's conversations: pinned first, then the most recently used.
    pub fn conversations(&self, bot: &str) -> Result<Vec<Conversation>> {
        let mut stmt = self.0.prepare_cached(&format!(
            "{CONVERSATION} WHERE bot = ?1 ORDER BY pinned DESC, touched_at DESC, rowid DESC"
        ))?;
        stmt.query_map([bot], conversation_row)?.collect()
    }

    pub fn conversation(&self, id: &str) -> Result<Option<Conversation>> {
        self.0
            .query_row(
                &format!("{CONVERSATION} WHERE id = ?1"),
                [id],
                conversation_row,
            )
            .optional()
    }

    /// The bot's current conversation: the one a prompt that names none goes to. The one last
    /// used, but not an archived one while another is not archived.
    pub fn current_conversation(&self, bot: &str) -> Result<String> {
        self.0.query_row(
            "SELECT id FROM conversations WHERE bot = ?1 ORDER BY archived, touched_at DESC, rowid DESC LIMIT 1",
            [bot],
            |r| r.get(0),
        )
    }

    /// A new conversation of the bot, which is then the one last used.
    pub fn new_conversation(&self, bot: &str, now: u64) -> Result<Conversation> {
        let id = random_hex(4);
        self.0.execute(
            "INSERT INTO conversations(id, bot, created_at, touched_at) VALUES (?1, ?2, ?3, ?3)",
            params![id, bot, now],
        )?;
        Ok(self.conversation(&id)?.expect("just inserted"))
    }

    /// What the person changed of a conversation; what is `None` stays. An empty `name` is no
    /// name: the title shows again.
    pub fn set_conversation(
        &self,
        id: &str,
        name: Option<&str>,
        pinned: Option<bool>,
        archived: Option<bool>,
    ) -> Result<()> {
        self.0
            .execute(
                "UPDATE conversations SET name = COALESCE(?2, name), pinned = COALESCE(?3, pinned), \
                 archived = COALESCE(?4, archived) WHERE id = ?1",
                params![id, name, pinned, archived],
            )
            .map(drop)
    }

    /// A prompt went into the conversation: it is the one last used, and no longer archived.
    pub fn touch_conversation(&self, id: &str, now: u64) -> Result<()> {
        self.0
            .execute(
                "UPDATE conversations SET touched_at = ?2, archived = 0 WHERE id = ?1",
                params![id, now],
            )
            .map(drop)
    }

    /// The bot has replied: a conversation without a title gets one from its first prompt.
    /// `true` when it got one.
    pub fn title_conversation(&self, id: &str) -> Result<bool> {
        give_title(&self.0, id)
    }

    /// A new conversation of the bot that begins as a copy of `from` up to, but not with, its
    /// prompt `nonce`: "fork from here". `from` is left as it was.
    pub fn fork_conversation(&self, from: &str, nonce: &str, now: u64) -> Result<Conversation> {
        let tx = self.0.unchecked_transaction()?;
        let id = random_hex(4);
        tx.execute(
            "INSERT INTO conversations(id, bot, created_at, touched_at) \
             SELECT ?1, bot, ?3, ?3 FROM conversations WHERE id = ?2",
            params![id, from, now],
        )?;
        tx.execute(
            "INSERT INTO transcript(conversation, at, entry) SELECT ?1, at, entry FROM transcript \
             WHERE conversation = ?2 AND seq < (SELECT min(seq) FROM transcript WHERE conversation = ?2 \
             AND json_extract(entry, '$.type') = 'prompt' AND json_extract(entry, '$.nonce') = ?3) ORDER BY seq",
            params![id, from, nonce],
        )?;
        give_title(&tx, &id)?;
        tx.commit()?;
        Ok(self.conversation(&id)?.expect("just inserted"))
    }

    /// Appends one transcript entry; returns its sequence number.
    pub fn append(&self, conversation: &str, at: u64, entry: &Value) -> Result<i64> {
        self.0.execute(
            "INSERT INTO transcript(conversation, at, entry) VALUES (?1, ?2, ?3)",
            params![conversation, at, json(entry)],
        )?;
        Ok(self.0.last_insert_rowid())
    }

    /// The newest `limit` prompts and replies holding `needle` (any case), of one conversation,
    /// of the conversations of `bot`, or of all. A reply is searched whole, as the chat shows
    /// it: a run of text pieces, found at the `seq` of its first.
    // shortcut: reads every transcript entry per search; add an index (FTS5) when that gets slow.
    pub fn search(
        &self,
        bot: Option<&str>,
        conversation: Option<&str>,
        needle: &str,
        limit: usize,
    ) -> Result<Vec<Hit>> {
        let mut stmt = self.0.prepare_cached(
            "SELECT t.seq, t.conversation, c.bot, t.at, t.entry FROM transcript t \
             JOIN conversations c ON c.id = t.conversation \
             WHERE (?1 IS NULL OR c.bot = ?1) AND (?2 IS NULL OR t.conversation = ?2) \
             ORDER BY t.conversation, t.seq",
        )?;
        let mut rows = stmt.query(params![bot, conversation])?;
        let needle = needle.to_lowercase();
        let mut hits = Vec::new();
        // The item being read, as `chat.ts` makes them: where it begins, and its words.
        let mut item: Option<(Hit, String)> = None;
        let done = |item: Option<(Hit, String)>, hits: &mut Vec<Hit>| {
            if let Some((mut hit, text)) = item
                && let Some(snippet) = snippet(&text, &needle)
            {
                hit.snippet = snippet;
                hits.push(hit);
            }
        };
        while let Some(row) = rows.next()? {
            let (seq, row_conversation): (i64, String) = (row.get(0)?, row.get(1)?);
            let entry: Value = parse(row.get(4)?)?;
            let same = item
                .as_ref()
                .is_some_and(|(h, _)| h.conversation == row_conversation);
            let text = |key: &str| entry[key].as_str().unwrap_or("").to_owned();
            let next = match entry["type"].as_str() {
                Some("prompt") => Some(("user", text("text"))),
                Some("text") => {
                    if let Some((Hit { kind: "reply", .. }, so_far)) =
                        item.as_mut().filter(|_| same)
                    {
                        so_far.push_str(entry["delta"].as_str().unwrap_or(""));
                        continue;
                    }
                    Some(("reply", text("delta")))
                }
                // Items the chat shows that are not searched; they end a reply.
                Some("thinking" | "tool_start") => Some(("other", String::new())),
                Some("turn_end") if entry["reason"] != "done" => Some(("other", String::new())),
                _ => None,
            };
            if !same {
                done(item.take(), &mut hits);
            }
            if let Some((kind, text)) = next {
                let hit = Hit {
                    bot: row.get(2)?,
                    conversation: row_conversation,
                    seq,
                    at_ms: row.get(3)?,
                    kind,
                    snippet: String::new(),
                };
                done(item.replace((hit, text)), &mut hits);
            }
        }
        done(item, &mut hits);
        hits.sort_by_key(|h| std::cmp::Reverse(h.seq));
        hits.truncate(limit);
        Ok(hits)
    }

    /// Removes the prompt entry with `nonce` and everything after it. Does nothing if there is none.
    /// The ledger keeps the removed prompts: a late retry of one must not undo the rewind.
    pub fn rewind_transcript(&self, conversation: &str, nonce: &str) -> Result<()> {
        self.0
            .execute(
                "DELETE FROM transcript WHERE conversation = ?1 AND seq >= (SELECT min(seq) FROM transcript
                 WHERE conversation = ?1 AND json_extract(entry, '$.type') = 'prompt' AND json_extract(entry, '$.nonce') = ?2)",
                [conversation, nonce],
            )
            .map(drop)
    }

    /// The last `limit` entries of a conversation before `before`, oldest first, and whether
    /// there are older ones.
    pub fn history(
        &self,
        conversation: &str,
        before: i64,
        limit: i64,
    ) -> Result<(Vec<Value>, bool)> {
        let mut stmt = self.0.prepare_cached(
            "SELECT seq, at, entry FROM transcript WHERE conversation = ?1 AND seq < ?2 ORDER BY seq DESC LIMIT ?3",
        )?;
        // One more than asked for tells whether there is more.
        let mut page = stmt
            .query_map(rusqlite::params![conversation, before, limit.saturating_add(1)], |r| {
                let entry: Value = parse(r.get(2)?)?;
                Ok(serde_json::json!({"seq": r.get::<_, i64>(0)?, "at": r.get::<_, i64>(1)?, "entry": entry}))
            })?
            .collect::<Result<Vec<_>>>()?;
        let more = page.len() as i64 > limit;
        page.truncate(limit as usize);
        page.reverse();
        Ok((page, more))
    }

    pub fn ledger(&self, bot: &str, nonce: &str) -> Result<Option<Ledger>> {
        self.0
            .query_row(
                "SELECT state FROM ledger WHERE bot = ?1 AND nonce = ?2",
                [bot, nonce],
                |r| r.get::<_, String>(0),
            )
            .optional()
            .map(|s| s.as_deref().map(Ledger::parse))
    }

    pub fn set_ledger(&self, bot: &str, nonce: &str, state: Ledger, now: u64) -> Result<()> {
        self.0
            .execute(
                "INSERT INTO ledger(bot, nonce, state, at) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(bot, nonce) DO UPDATE SET state = excluded.state, at = excluded.at",
                params![bot, nonce, state.as_str(), now],
            )
            .map(drop)
    }

    /// Forgets a nonce the engine refused, so the same nonce may be sent again.
    pub fn forget_ledger(&self, bot: &str, nonce: &str) -> Result<()> {
        self.0
            .execute(
                "DELETE FROM ledger WHERE bot = ?1 AND nonce = ?2",
                [bot, nonce],
            )
            .map(drop)
    }

    /// The bot's engine stopped: what it may or may not have received is uncertain.
    pub fn ledger_engine_stopped(&self, bot: &str) -> Result<()> {
        self.0
            .execute(
                "UPDATE ledger SET state = 'uncertain' WHERE bot = ?1 AND state = 'sending'",
                [bot],
            )
            .map(drop)
    }

    /// Prompts waiting for the bot, in the conversations of the bot, in the order they go.
    pub fn queue(&self, bot: &str) -> Result<Vec<Queued>> {
        let mut stmt = self.0.prepare_cached(
            "SELECT q.conversation, q.nonce, q.text FROM queue q JOIN conversations c ON c.id = q.conversation \
             WHERE c.bot = ?1 ORDER BY q.seq",
        )?;
        stmt.query_map([bot], |r| {
            Ok(Queued {
                conversation: r.get(0)?,
                nonce: r.get(1)?,
                text: r.get(2)?,
            })
        })?
        .collect()
    }

    /// Bots with a prompt waiting.
    pub fn queued_bots(&self) -> Result<Vec<String>> {
        let mut stmt = self.0.prepare_cached(
            "SELECT DISTINCT c.bot FROM queue q JOIN conversations c ON c.id = q.conversation",
        )?;
        stmt.query_map([], |r| r.get(0))?.collect()
    }

    /// Adds a prompt to a conversation, at the back, or (`front`) as the next one to go.
    pub fn enqueue(&self, conversation: &str, nonce: &str, text: &str, front: bool) -> Result<()> {
        let seq = if front {
            "(SELECT COALESCE(MIN(seq), 1) - 1 FROM queue)"
        } else {
            "(SELECT COALESCE(MAX(seq), 0) + 1 FROM queue)"
        };
        self.0
            .execute(
                &format!(
                    "INSERT INTO queue(seq, conversation, nonce, text) VALUES ({seq}, ?1, ?2, ?3)"
                ),
                [conversation, nonce, text],
            )
            .map(drop)
    }

    /// Removes a waiting prompt of the bot; the conversation it waited in, or `None` if it was
    /// not waiting.
    pub fn dequeue(&self, bot: &str, nonce: &str) -> Result<Option<String>> {
        self.0
            .query_row(
                "DELETE FROM queue WHERE nonce = ?2 AND conversation IN (SELECT id FROM conversations WHERE bot = ?1) \
                 RETURNING conversation",
                [bot, nonce],
                |r| r.get(0),
            )
            .optional()
    }

    /// Stores a message; its number. `parent` is the message its sender was working on, if any.
    pub fn put_mail(
        &self,
        from: &str,
        to: &str,
        text: &str,
        state: &str,
        at: u64,
        parent: Option<i64>,
    ) -> Result<i64> {
        self.0.execute(
            "INSERT INTO mail(from_bot, to_bot, text, state, at, parent) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![from, to, text, state, at, parent],
        )?;
        Ok(self.0.last_insert_rowid())
    }

    /// The bots that asked for the work the message `seq` is: its sender, then the sender of the
    /// message that sender was working on, and so on; nearest first. A bot that was deleted is
    /// not in it (its messages went with it).
    pub fn mail_askers(&self, seq: i64) -> Result<Vec<String>> {
        let mut stmt = self.0.prepare_cached(
            "WITH RECURSIVE up(seq, bot, parent) AS (\
                 SELECT seq, from_bot, parent FROM mail WHERE seq = ?1 \
                 UNION ALL SELECT m.seq, m.from_bot, m.parent FROM mail m JOIN up ON m.seq = up.parent) \
             SELECT bot FROM up ORDER BY seq DESC",
        )?;
        stmt.query_map([seq], |r| r.get(0))?.collect()
    }

    pub fn mail(&self, seq: i64) -> Result<Option<Mail>> {
        self.0
            .query_row(&format!("{MAIL} WHERE seq = ?1"), [seq], mail_row)
            .optional()
    }

    /// The messages in `state`, oldest first; for one bot `to` if given.
    pub fn mail_in(&self, state: &str, to: Option<&str>) -> Result<Vec<Mail>> {
        let mut stmt = self.0.prepare_cached(&format!(
            "{MAIL} WHERE state = ?1 AND (?2 IS NULL OR to_bot = ?2) ORDER BY seq"
        ))?;
        stmt.query_map(params![state, to], mail_row)?.collect()
    }

    /// The last 50 messages the bot sent or got, oldest first.
    pub fn mail_of(&self, bot: &str) -> Result<Vec<Mail>> {
        let mut stmt = self.0.prepare_cached(&format!(
            "SELECT * FROM ({MAIL} WHERE from_bot = ?1 OR to_bot = ?1 ORDER BY seq DESC LIMIT 50) ORDER BY seq"
        ))?;
        stmt.query_map([bot], mail_row)?.collect()
    }

    pub fn set_mail(
        &self,
        seq: i64,
        state: &str,
        answer: Option<&str>,
        error: Option<&str>,
    ) -> Result<()> {
        self.0
            .execute(
                "UPDATE mail SET state = ?2, answer = ?3, error = ?4 WHERE seq = ?1",
                params![seq, state, answer, error],
            )
            .map(drop)
    }

    pub fn put_question(&self, conversation: &str, question: &str, entry: &Value) -> Result<()> {
        self.0
            .execute(
                "INSERT OR REPLACE INTO questions(conversation, question, entry) VALUES (?1, ?2, ?3)",
                params![conversation, question, json(entry)],
            )
            .map(drop)
    }

    /// Removes an open question; `false` if it was not open.
    pub fn take_question(&self, conversation: &str, question: &str) -> Result<bool> {
        Ok(self.0.execute(
            "DELETE FROM questions WHERE conversation = ?1 AND question = ?2",
            [conversation, question],
        )? > 0)
    }

    pub fn questions(&self, conversation: &str) -> Result<Vec<Value>> {
        let mut stmt = self
            .0
            .prepare_cached("SELECT entry FROM questions WHERE conversation = ?1 ORDER BY rowid")?;
        stmt.query_map([conversation], |r| parse(r.get(0)?))?
            .collect()
    }

    /// The person has looked at the conversation up to its newest entry. `false` when they had
    /// already.
    pub fn mark_seen(&self, conversation: &str) -> Result<bool> {
        Ok(self.0.execute(
            "INSERT INTO seen(conversation, seq) SELECT ?1, COALESCE(max(seq), 0) FROM transcript WHERE conversation = ?1 \
             ON CONFLICT(conversation) DO UPDATE SET seq = excluded.seq WHERE seq < excluded.seq",
            [conversation],
        )? > 0)
    }

    /// The `turn_end` entry of the last turn that ended after the person last looked at the
    /// conversation; `None` when it was the person who stopped it.
    pub fn unseen_end(&self, conversation: &str) -> Result<Option<Value>> {
        let end: Option<String> = self
            .0
            .query_row(
                "SELECT entry FROM transcript WHERE conversation = ?1 \
                 AND seq > COALESCE((SELECT seq FROM seen WHERE conversation = ?1), 0) \
                 AND json_extract(entry, '$.type') = 'turn_end' ORDER BY seq DESC LIMIT 1",
                [conversation],
                |r| r.get(0),
            )
            .optional()?;
        let end: Option<Value> = end.map(parse).transpose()?;
        Ok(end.filter(|e| e["reason"] != "aborted"))
    }

    /// When the conversation last changed, and the last line of the bot's latest reply in it.
    // shortcut: looks at the last 200 entries only; a reply further back is not shown.
    pub fn tail(&self, conversation: &str) -> Result<(u64, String)> {
        let mut stmt = self.0.prepare_cached(
            "SELECT at, entry FROM transcript WHERE conversation = ?1 ORDER BY seq DESC LIMIT 200",
        )?;
        let (mut at, mut said, mut reading) = (0, Vec::new(), false);
        for row in stmt.query_map([conversation], |r| {
            Ok((r.get::<_, u64>(0)?, parse::<Value>(r.get(1)?)?))
        })? {
            let (when, entry) = row?;
            at = at.max(when);
            match entry["type"].as_str() {
                Some("text") => {
                    reading = true;
                    said.push(entry["delta"].as_str().unwrap_or_default().to_owned());
                }
                // The reply is the run of text just before it; a prompt is an older turn.
                Some("prompt") => break,
                _ if reading => break,
                _ => {}
            }
        }
        said.reverse();
        let said = said.concat();
        let last = said
            .lines()
            .map(str::trim)
            .rfind(|l| !l.is_empty())
            .unwrap_or_default();
        Ok((at, last.chars().take(300).collect()))
    }

    /// Whether a question is open in any conversation of the bot.
    pub fn has_questions(&self, bot: &str) -> Result<bool> {
        self.0.query_row(
            "SELECT EXISTS(SELECT 1 FROM questions q JOIN conversations c ON c.id = q.conversation WHERE c.bot = ?1)",
            [bot],
            |r| r.get(0),
        )
    }

    /// The bot's engine stopped: none of its conversations has an open question.
    pub fn clear_questions(&self, bot: &str) -> Result<()> {
        self.0
            .execute(
                "DELETE FROM questions WHERE conversation IN (SELECT id FROM conversations WHERE bot = ?1)",
                [bot],
            )
            .map(drop)
    }

    pub fn routines(&self, bot: &str) -> Result<Vec<Routine>> {
        let mut stmt = self
            .0
            .prepare_cached("SELECT routine FROM routines WHERE bot = ?1")?;
        stmt.query_map([bot], |r| parse(r.get(0)?))?.collect()
    }

    pub fn all_routines(&self) -> Result<Vec<(String, Routine)>> {
        let mut stmt = self.0.prepare_cached("SELECT bot, routine FROM routines")?;
        stmt.query_map([], |r| Ok((r.get(0)?, parse(r.get(1)?)?)))?
            .collect()
    }

    /// Creates or replaces by `routine.id`, which must be set.
    pub fn put_routine(&self, bot: &str, routine: &Routine) -> Result<()> {
        let id = routine.id.as_deref().expect("stored routines have an id");
        self.0
            .execute(
                "INSERT OR REPLACE INTO routines(bot, id, routine) VALUES (?1, ?2, ?3)",
                params![bot, id, json(routine)],
            )
            .map(drop)
    }

    /// Deletes a routine and its past runs.
    pub fn delete_routine(&self, bot: &str, id: &str) -> Result<bool> {
        self.0.execute(
            "DELETE FROM routine_runs WHERE bot = ?1 AND routine = ?2",
            [bot, id],
        )?;
        Ok(self
            .0
            .execute("DELETE FROM routines WHERE bot = ?1 AND id = ?2", [bot, id])?
            > 0)
    }

    /// Stores `routine` together with its `last_run` as a past run, in one transaction: a crash
    /// cannot keep the run and lose the routine's next slot. `replace_last`: the run takes the
    /// place of the newest one (a prompt recorded as sent that the engine then refused).
    pub fn put_run(&self, bot: &str, routine: &Routine, replace_last: bool) -> Result<()> {
        let id = routine.id.as_deref().expect("stored routines have an id");
        let run = routine.last_run.as_ref().expect("the run to record");
        let tx = self.0.unchecked_transaction()?;
        if replace_last {
            self.replace_last_run(bot, id, run)?;
        } else {
            self.add_run(bot, id, run)?;
        }
        self.put_routine(bot, routine)?;
        tx.commit()
    }

    /// Adds a run to a routine's history, which keeps the newest `MAX_RUNS`.
    fn add_run(&self, bot: &str, routine: &str, run: &RoutineRun) -> Result<()> {
        self.0.execute(
            "INSERT INTO routine_runs(bot, routine, run) VALUES (?1, ?2, ?3)",
            params![bot, routine, json(run)],
        )?;
        self.0
            .execute(
                "DELETE FROM routine_runs WHERE bot = ?1 AND routine = ?2 AND seq <=
                 (SELECT seq FROM routine_runs WHERE bot = ?1 AND routine = ?2 ORDER BY seq DESC LIMIT 1 OFFSET ?3)",
                params![bot, routine, MAX_RUNS],
            )
            .map(drop)
    }

    fn replace_last_run(&self, bot: &str, routine: &str, run: &RoutineRun) -> Result<()> {
        self.0
            .execute(
                "UPDATE routine_runs SET run = ?3 WHERE seq =
                 (SELECT max(seq) FROM routine_runs WHERE bot = ?1 AND routine = ?2)",
                params![bot, routine, json(run)],
            )
            .map(drop)
    }

    /// A routine's past runs, newest first.
    pub fn runs(&self, bot: &str, routine: &str) -> Result<Vec<RoutineRun>> {
        let mut stmt = self.0.prepare_cached(
            "SELECT run FROM routine_runs WHERE bot = ?1 AND routine = ?2 ORDER BY seq DESC",
        )?;
        stmt.query_map([bot, routine], |r| parse(r.get(0)?))?
            .collect()
    }

    /// Whether the bot may reach the whole disk. Unless the user said so, it is kept in its folder.
    pub fn whole_disk(&self, bot: &str) -> Result<bool> {
        let stored = self
            .0
            .query_row("SELECT whole_disk FROM access WHERE bot = ?1", [bot], |r| {
                r.get(0)
            })
            .optional()?;
        Ok(stored.unwrap_or(false))
    }

    /// The hosts the bot's commands may reach, or `None` if the network is open to them (the
    /// default).
    pub fn network(&self, bot: &str) -> Result<Option<Vec<String>>> {
        self.0
            .query_row("SELECT hosts FROM network WHERE bot = ?1", [bot], |r| {
                parse(r.get(0)?)
            })
            .optional()
    }

    pub fn set_network(&self, bot: &str, hosts: Option<&[String]>) -> Result<()> {
        match hosts {
            Some(hosts) => self.0.execute(
                "INSERT OR REPLACE INTO network(bot, hosts) VALUES (?1, ?2)",
                params![bot, json(&hosts)],
            ),
            None => self.0.execute("DELETE FROM network WHERE bot = ?1", [bot]),
        }
        .map(drop)
    }

    pub fn set_whole_disk(&self, bot: &str, whole_disk: bool) -> Result<()> {
        self.0
            .execute(
                "INSERT OR REPLACE INTO access(bot, whole_disk) VALUES (?1, ?2)",
                params![bot, whole_disk],
            )
            .map(drop)
    }

    /// Whether any bot runs on an installed agent.
    pub fn any_agent(&self) -> Result<bool> {
        self.0
            .query_row("SELECT EXISTS(SELECT 1 FROM agents)", [], |r| r.get(0))
    }

    /// The installed agent the bot runs on, as a program and its arguments. None: Bate's own
    /// engine.
    pub fn agent(&self, bot: &str) -> Result<Option<Vec<String>>> {
        let stored: Option<String> = self
            .0
            .query_row("SELECT command FROM agents WHERE bot = ?1", [bot], |r| {
                r.get(0)
            })
            .optional()?;
        stored.map(parse).transpose()
    }

    pub fn set_agent(&self, bot: &str, command: Option<&[String]>) -> Result<()> {
        let done = match command {
            Some(command) => self.0.execute(
                "INSERT OR REPLACE INTO agents(bot, command) VALUES (?1, ?2)",
                params![bot, json(&command)],
            ),
            None => self.0.execute("DELETE FROM agents WHERE bot = ?1", [bot]),
        };
        done.map(drop)
    }

    /// What the bot may do with actions of `kind`; the kind's default if the user never chose.
    pub fn level(&self, bot: &str, kind: Kind) -> Result<Level> {
        let stored = self
            .0
            .query_row(
                "SELECT level FROM permissions WHERE bot = ?1 AND kind = ?2",
                params![bot, json(&kind)],
                |r| parse(r.get(0)?),
            )
            .optional()?;
        Ok(stored.unwrap_or(kind.default_level()))
    }

    pub fn set_level(&self, bot: &str, kind: Kind, level: Level) -> Result<()> {
        self.0
            .execute(
                "INSERT OR REPLACE INTO permissions(bot, kind, level) VALUES (?1, ?2, ?3)",
                params![bot, json(&kind), json(&level)],
            )
            .map(drop)
    }

    /// Whether an "Always" answer covers exactly this action.
    pub fn has_grant(&self, bot: &str, kind: Kind, target: &str) -> Result<bool> {
        self.0
            .prepare_cached("SELECT 1 FROM grants WHERE bot = ?1 AND kind = ?2 AND target = ?3")?
            .exists(params![bot, json(&kind), target])
    }

    pub fn put_grant(&self, bot: &str, grant: &Grant) -> Result<()> {
        self.0
            .execute(
                "INSERT OR REPLACE INTO grants(bot, kind, target, at) VALUES (?1, ?2, ?3, ?4)",
                params![bot, json(&grant.kind), grant.target, grant.at_ms],
            )
            .map(drop)
    }

    /// A bot's grants, oldest first.
    pub fn grants(&self, bot: &str) -> Result<Vec<Grant>> {
        let mut stmt = self.0.prepare_cached(
            "SELECT kind, target, at FROM grants WHERE bot = ?1 ORDER BY at, target",
        )?;
        stmt.query_map([bot], |r| {
            Ok(Grant {
                kind: parse(r.get(0)?)?,
                target: r.get(1)?,
                at_ms: r.get(2)?,
            })
        })?
        .collect()
    }

    pub fn delete_grant(&self, bot: &str, kind: Kind, target: &str) -> Result<bool> {
        Ok(self.0.execute(
            "DELETE FROM grants WHERE bot = ?1 AND kind = ?2 AND target = ?3",
            params![bot, json(&kind), target],
        )? > 0)
    }

    /// Whether an "Allow in this conversation" answer in `conversation` covers exactly this action.
    pub fn has_conversation_grant(
        &self,
        conversation: &str,
        kind: Kind,
        target: &str,
    ) -> Result<bool> {
        self.0
            .prepare_cached(
                "SELECT 1 FROM conversation_grants WHERE conversation = ?1 AND kind = ?2 AND target = ?3",
            )?
            .exists(params![conversation, json(&kind), target])
    }

    pub fn put_conversation_grant(&self, conversation: &str, grant: &Grant) -> Result<()> {
        self.0
            .execute(
                "INSERT OR REPLACE INTO conversation_grants(conversation, kind, target, at) VALUES (?1, ?2, ?3, ?4)",
                params![conversation, json(&grant.kind), grant.target, grant.at_ms],
            )
            .map(drop)
    }

    /// The grants of all of a bot's conversations, oldest first, each with its conversation's id and
    /// name (what the person called it, else its title).
    pub fn conversation_grants(&self, bot: &str) -> Result<Vec<(String, String, Grant)>> {
        let mut stmt = self.0.prepare_cached(
            "SELECT c.id, COALESCE(NULLIF(c.name, ''), c.title), g.kind, g.target, g.at \
             FROM conversation_grants g JOIN conversations c ON c.id = g.conversation \
             WHERE c.bot = ?1 ORDER BY g.at, g.target",
        )?;
        stmt.query_map([bot], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                Grant {
                    kind: parse(r.get(2)?)?,
                    target: r.get(3)?,
                    at_ms: r.get(4)?,
                },
            ))
        })?
        .collect()
    }

    /// Removes one conversation grant; whether there was one of the bot's.
    pub fn delete_conversation_grant(
        &self,
        bot: &str,
        conversation: &str,
        kind: Kind,
        target: &str,
    ) -> Result<bool> {
        Ok(self.0.execute(
            "DELETE FROM conversation_grants WHERE conversation = ?1 AND kind = ?2 AND target = ?3 \
             AND conversation IN (SELECT id FROM conversations WHERE bot = ?4)",
            params![conversation, json(&kind), target, bot],
        )? > 0)
    }

    /// Whether the bot's connector tool `tool` (`<connector>__<tool>`) is blocked.
    pub fn is_blocked(&self, bot: &str, tool: &str) -> Result<bool> {
        self.0
            .prepare_cached("SELECT 1 FROM blocked_tools WHERE bot = ?1 AND tool = ?2")?
            .exists(params![bot, tool])
    }

    /// The bot's blocked connector tools, by name.
    pub fn blocked_tools(&self, bot: &str) -> Result<Vec<String>> {
        let mut stmt = self
            .0
            .prepare_cached("SELECT tool FROM blocked_tools WHERE bot = ?1 ORDER BY tool")?;
        stmt.query_map([bot], |r| r.get(0))?.collect()
    }

    /// Blocks or unblocks a tool. Blocked wins over an "Always" grant, which is dropped here so
    /// that unblocking asks again.
    pub fn set_blocked(&self, bot: &str, tool: &str, blocked: bool) -> Result<()> {
        if blocked {
            self.0.execute(
                "INSERT OR IGNORE INTO blocked_tools(bot, tool) VALUES (?1, ?2)",
                params![bot, tool],
            )?;
            self.delete_grant(bot, Kind::Connector, tool)?;
        } else {
            self.0.execute(
                "DELETE FROM blocked_tools WHERE bot = ?1 AND tool = ?2",
                params![bot, tool],
            )?;
        }
        Ok(())
    }

    /// Forgets the blocked tools of connector `name`; returns how many.
    pub fn delete_connector_blocked(&self, bot: &str, name: &str) -> Result<usize> {
        let prefix = format!("{name}__");
        self.0.execute(
            "DELETE FROM blocked_tools WHERE bot = ?1 AND substr(tool, 1, length(?2)) = ?2",
            params![bot, prefix],
        )
    }

    /// Removes the grants, for good and for one conversation, for the tools of connector `name`;
    /// returns how many.
    pub fn delete_connector_grants(&self, bot: &str, name: &str) -> Result<usize> {
        let prefix = format!("{name}__");
        let forever = self.0.execute(
            "DELETE FROM grants WHERE bot = ?1 AND kind = ?2 AND substr(target, 1, length(?3)) = ?3",
            params![bot, json(&Kind::Connector), prefix],
        )?;
        let here = self.0.execute(
            "DELETE FROM conversation_grants WHERE kind = ?2 AND substr(target, 1, length(?3)) = ?3 \
             AND conversation IN (SELECT id FROM conversations WHERE bot = ?1)",
            params![bot, json(&Kind::Connector), prefix],
        )?;
        Ok(forever + here)
    }

    pub fn connectors(&self, bot: &str) -> Result<Vec<Connector>> {
        let mut stmt = self
            .0
            .prepare_cached("SELECT connector FROM connectors WHERE bot = ?1 ORDER BY name")?;
        stmt.query_map([bot], |r| parse(r.get(0)?))?.collect()
    }

    pub fn put_connector(&self, bot: &str, c: &Connector) -> Result<()> {
        self.0
            .execute(
                "INSERT OR REPLACE INTO connectors(bot, name, connector) VALUES (?1, ?2, ?3)",
                params![bot, c.name, json(c)],
            )
            .map(drop)
    }

    /// A project as stored: the bots and the lead come from the `bots` and `lead` columns.
    fn project_row(&self, r: &rusqlite::Row) -> Result<Project> {
        let mut project: Project = parse(r.get(0)?)?;
        project.lead = r.get(1)?;
        project.bots = self
            .0
            .prepare_cached("SELECT id FROM bots WHERE project = ?1 ORDER BY id")?
            .query_map([r.get::<_, String>(2)?], |r| r.get(0))?
            .collect::<Result<_>>()?;
        Ok(project)
    }

    pub fn projects(&self) -> Result<Vec<Project>> {
        let mut stmt = self
            .0
            .prepare_cached("SELECT project, lead, id FROM projects ORDER BY id")?;
        stmt.query_map([], |r| self.project_row(r))?.collect()
    }

    /// The project `bot` is in.
    pub fn project_of(&self, bot: &str) -> Result<Option<Project>> {
        self.0
            .prepare_cached(
                "SELECT p.project, p.lead, p.id FROM bots b JOIN projects p ON p.id = b.project \
                 WHERE b.id = ?1",
            )?
            .query_row([bot], |r| self.project_row(r))
            .optional()
    }

    /// Creates or replaces by `project.id`, which must be set. The database refuses a bot that
    /// does not exist, one that is in another project, and a lead that is not one of the bots.
    pub fn put_project(&self, project: &Project) -> Result<()> {
        let id = project.id.as_deref().expect("stored projects have an id");
        let members = json(&project.bots);
        let stored = Project {
            bots: Vec::new(),
            lead: None,
            ..project.clone()
        };
        let tx = self.0.unchecked_transaction()?;
        // Not `INSERT OR REPLACE`: replacing deletes the row, which clears its bots' place.
        tx.execute(
            "INSERT INTO projects(id, project, lead) VALUES (?1, ?2, ?3) \
             ON CONFLICT(id) DO UPDATE SET project = ?2, lead = ?3",
            params![id, json(&stored), project.lead],
        )?;
        tx.execute(
            "UPDATE bots SET project = NULL \
             WHERE project = ?1 AND id NOT IN (SELECT value FROM json_each(?2))",
            [id, &members],
        )?;
        let placed = tx.execute(
            "UPDATE bots SET project = ?1 WHERE id IN (SELECT value FROM json_each(?2)) \
             AND (project IS NULL OR project = ?1)",
            [id, &members],
        )?;
        let lead_in = project
            .lead
            .as_ref()
            .is_none_or(|l| project.bots.contains(l));
        if placed != project.bots.len() || !lead_in {
            return Err(refused(
                "a bot is in at most one project, and the lead is one of its bots",
            ));
        }
        tx.commit()
    }

    pub fn workplaces(&self) -> Result<Vec<Workplace>> {
        let mut stmt = self.0.prepare_cached(
            "SELECT w.name, w.version, w.fingerprint, a.sha256 FROM workplaces w \
             LEFT JOIN workplace_archives a ON a.name = w.name ORDER BY w.name",
        )?;
        stmt.query_map([], |r| {
            Ok(Workplace {
                name: r.get(0)?,
                version: r.get(1)?,
                fingerprint: r.get(2)?,
                archive: r.get(3)?,
            })
        })?
        .collect()
    }

    pub fn workplace(&self, name: &str) -> Result<Option<Workplace>> {
        Ok(self.workplaces()?.into_iter().find(|w| w.name == name))
    }

    pub fn put_workplace(&self, w: &Workplace) -> Result<()> {
        let tx = self.0.unchecked_transaction()?;
        tx.execute(
            "INSERT OR REPLACE INTO workplaces(name, version, fingerprint) VALUES (?1, ?2, ?3)",
            params![w.name, w.version, w.fingerprint],
        )?;
        match &w.archive {
            Some(sha256) => tx.execute(
                "INSERT OR REPLACE INTO workplace_archives(name, sha256) VALUES (?1, ?2)",
                params![w.name, sha256],
            )?,
            None => tx.execute("DELETE FROM workplace_archives WHERE name = ?1", [&w.name])?,
        };
        tx.commit()
    }

    pub fn delete_workplace(&self, name: &str) -> Result<()> {
        let tx = self.0.unchecked_transaction()?;
        tx.execute("DELETE FROM workplaces WHERE name = ?1", [name])?;
        tx.execute("DELETE FROM workplace_archives WHERE name = ?1", [name])?;
        tx.commit()
    }

    /// Deletes the project and its room; its bots stay, in no project.
    pub fn delete_project(&self, id: &str) -> Result<()> {
        let tx = self.0.unchecked_transaction()?;
        tx.execute("DELETE FROM room WHERE project = ?1", [id])?;
        tx.execute("DELETE FROM projects WHERE id = ?1", [id])?;
        tx.commit()
    }

    /// Adds a line to the project's room; its number.
    pub fn put_room(
        &self,
        project: &str,
        from: &str,
        to: &str,
        text: &str,
        at: u64,
    ) -> Result<i64> {
        self.0.execute(
            "INSERT INTO room(project, author, to_bot, text, at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![project, from, to, text, at],
        )?;
        Ok(self.0.last_insert_rowid())
    }

    pub fn room_entry(&self, seq: i64) -> Result<Option<(String, RoomEntry)>> {
        self.0
            .query_row(
                "SELECT project, seq, author, to_bot, text, at FROM room WHERE seq = ?1",
                [seq],
                |r| {
                    Ok((
                        r.get(0)?,
                        RoomEntry {
                            seq: r.get(1)?,
                            from: r.get(2)?,
                            to: r.get(3)?,
                            text: r.get(4)?,
                            at_ms: r.get(5)?,
                        },
                    ))
                },
            )
            .optional()
    }

    /// The last `limit` lines of the project's room, oldest first.
    pub fn room(&self, project: &str, limit: usize) -> Result<Vec<RoomEntry>> {
        let mut stmt = self.0.prepare_cached(&format!(
            "SELECT * FROM ({ROOM} WHERE project = ?1 ORDER BY seq DESC LIMIT ?2) ORDER BY seq"
        ))?;
        stmt.query_map(params![project, limit as i64], room_row)?
            .collect()
    }

    /// What `bot` has not seen of the project's room, newest first: lines since its own last
    /// one, not its own and not addressed to it (those reach it as prompts).
    pub fn room_unseen(&self, project: &str, bot: &str, limit: usize) -> Result<Vec<RoomEntry>> {
        let mut stmt = self.0.prepare_cached(&format!(
            "{ROOM} WHERE project = ?1 AND author != ?2 AND to_bot != ?2 AND seq > \
             (SELECT COALESCE(MAX(seq), 0) FROM room WHERE project = ?1 AND author = ?2) \
             ORDER BY seq DESC LIMIT ?3"
        ))?;
        stmt.query_map(params![project, bot, limit as i64], room_row)?
            .collect()
    }

    /// The key under which the engine reaches this connector through the service; `fresh` is
    /// kept when there is none yet.
    pub fn oauth_key(&self, bot: &str, name: &str, fresh: &str) -> Result<String> {
        self.0.execute(
            "INSERT OR IGNORE INTO oauth(bot, name, key) VALUES (?1, ?2, ?3)",
            [bot, name, fresh],
        )?;
        self.0.query_row(
            "SELECT key FROM oauth WHERE bot = ?1 AND name = ?2",
            [bot, name],
            |r| r.get(0),
        )
    }

    /// The bot and connector a key is for.
    pub fn oauth_by_key(&self, key: &str) -> Result<Option<(String, String)>> {
        self.0
            .query_row("SELECT bot, name FROM oauth WHERE key = ?1", [key], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .optional()
    }

    /// The sign-in of a connector, if the user is signed in.
    pub fn tokens(&self, bot: &str, name: &str) -> Result<Option<Tokens>> {
        let row: Option<Option<String>> = self
            .0
            .query_row(
                "SELECT tokens FROM oauth WHERE bot = ?1 AND name = ?2",
                [bot, name],
                |r| r.get(0),
            )
            .optional()?;
        let Some(legacy) = row else { return Ok(None) };
        let text = secret(&oauth_account(bot, name), legacy.unwrap_or_default())?;
        Some(text).filter(|t| !t.is_empty()).map(parse).transpose()
    }

    /// Keeps a sign-in, or forgets it (`None`). False if the connector has no key.
    pub fn set_tokens(&self, bot: &str, name: &str, tokens: Option<&Tokens>) -> Result<bool> {
        let known = self.0.query_row(
            "SELECT 1 FROM oauth WHERE bot = ?1 AND name = ?2",
            [bot, name],
            |_| Ok(()),
        );
        if known.optional()?.is_none() {
            return Ok(false);
        }
        let kept = keep(
            &oauth_account(bot, name),
            &tokens.map(json).unwrap_or_default(),
        )?;
        self.0.execute(
            "UPDATE oauth SET tokens = ?3 WHERE bot = ?1 AND name = ?2",
            params![bot, name, kept],
        )?;
        Ok(true)
    }

    pub fn delete_oauth(&self, bot: &str, name: &str) -> Result<()> {
        kc(keychain::delete(&oauth_account(bot, name)))?;
        self.0
            .execute(
                "DELETE FROM oauth WHERE bot = ?1 AND name = ?2",
                [bot, name],
            )
            .map(drop)
    }

    /// What the bot has been given a secret for: the name and the host. Never the value.
    pub fn secrets(&self, bot: &str) -> Result<Vec<(String, String)>> {
        let mut stmt = self
            .0
            .prepare_cached("SELECT name, site FROM secrets WHERE bot = ?1 ORDER BY name")?;
        stmt.query_map([bot], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect()
    }

    /// The value of the bot's secret `name`, from the Keychain; `None` when it has none.
    pub fn secret_value(&self, bot: &str, name: &str) -> Result<Option<String>> {
        kc(keychain::get(&secret_account(bot, name)))
    }

    /// Keeps a secret the person typed, for `site`, and forgets an earlier one of that name.
    /// The value goes to the Keychain and nowhere else: unlike `keep`, a Keychain that refuses
    /// it fails the call, since a value in `app.db` is what this must never be.
    pub fn put_secret(&self, bot: &str, name: &str, site: &str, value: &str) -> Result<()> {
        kc(keychain::put(&secret_account(bot, name), value))?;
        self.0
            .execute(
                "INSERT INTO secrets(bot, name, site) VALUES (?1, ?2, ?3) \
                 ON CONFLICT(bot, name) DO UPDATE SET site = ?3",
                [bot, name, site],
            )
            .map(drop)
    }

    /// Forgets one secret. False when the bot has none of that name.
    pub fn delete_secret(&self, bot: &str, name: &str) -> Result<bool> {
        let known = self.0.query_row(
            "SELECT 1 FROM secrets WHERE bot = ?1 AND name = ?2",
            [bot, name],
            |_| Ok(()),
        );
        if known.optional()?.is_none() {
            return Ok(false);
        }
        kc(keychain::delete(&secret_account(bot, name)))?;
        self.0.execute(
            "DELETE FROM secrets WHERE bot = ?1 AND name = ?2",
            [bot, name],
        )?;
        Ok(true)
    }

    /// The names of the bot's skills that are switched off, by name.
    pub fn skills_off(&self, bot: &str) -> Result<Vec<String>> {
        let mut stmt = self
            .0
            .prepare_cached("SELECT name FROM skills_off WHERE bot = ?1 ORDER BY name")?;
        stmt.query_map([bot], |r| r.get(0))?.collect()
    }

    /// Switches the skill `name` of `bot` off or on again.
    pub fn set_skill_off(&self, bot: &str, name: &str, off: bool) -> Result<()> {
        if off {
            self.0.execute(
                "INSERT OR IGNORE INTO skills_off(bot, name) VALUES (?1, ?2)",
                [bot, name],
            )?;
        } else {
            self.0.execute(
                "DELETE FROM skills_off WHERE bot = ?1 AND name = ?2",
                [bot, name],
            )?;
        }
        Ok(())
    }

    pub fn delete_connector(&self, bot: &str, name: &str) -> Result<bool> {
        Ok(self.0.execute(
            "DELETE FROM connectors WHERE bot = ?1 AND name = ?2",
            [bot, name],
        )? > 0)
    }

    /// Keeps a copy and drops the owner's oldest ones while they hold more than
    /// `proto::undo::MAX_KEPT` (a copy counts its bytes and `ENTRY` more).
    pub fn keep(&self, owner: Owner, k: &Kept) -> Result<()> {
        let (bot, project) = match owner {
            Owner::Bot(id) => (Some(id), None),
            Owner::Project(id) => (None, Some(id)),
        };
        let tx = self.0.unchecked_transaction()?;
        tx.execute(
            "INSERT INTO file_copies(id, bot, project, conversation, path, earlier, after_sum, who, at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![k.id, bot, project, k.conversation, k.path, k.earlier, k.after, k.who, k.at],
        )?;
        tx.execute(
            "DELETE FROM file_copies WHERE n IN (SELECT n FROM (SELECT n, \
             SUM(COALESCE(LENGTH(earlier), 0) + ?3) OVER (ORDER BY n DESC) AS total \
             FROM file_copies WHERE bot IS ?1 AND project IS ?2) WHERE total > ?4)",
            params![bot, project, proto::undo::ENTRY, proto::undo::MAX_KEPT],
        )?;
        tx.commit()
    }

    /// The copy `id`, whoever owns it.
    pub fn kept(&self, id: &str) -> Result<Option<Kept>> {
        self.0
            .query_row(
                "SELECT id, bot, conversation, path, earlier, after_sum, who, at, undone_at \
                 FROM file_copies WHERE id = ?1",
                [id],
                |r| {
                    Ok(Kept {
                        id: r.get(0)?,
                        bot: r.get(1)?,
                        conversation: r.get(2)?,
                        path: r.get(3)?,
                        earlier: r.get(4)?,
                        after: r.get(5)?,
                        who: if r.get::<_, String>(6)? == "bot" {
                            "bot"
                        } else {
                            "user"
                        },
                        at: r.get(7)?,
                        undone: r.get::<_, Option<u64>>(8)?.is_some(),
                    })
                },
            )
            .optional()
    }

    pub fn set_undone(&self, id: &str, at: u64) -> Result<()> {
        self.0
            .execute(
                "UPDATE file_copies SET undone_at = ?2 WHERE id = ?1",
                params![id, at],
            )
            .map(drop)
    }

    /// The earlier versions of the file at `path` (those with content), newest first.
    pub fn versions(&self, path: &str) -> Result<Vec<Version>> {
        let mut stmt = self.0.prepare_cached(
            "SELECT id, at, LENGTH(earlier), who FROM file_copies \
             WHERE path = ?1 AND earlier IS NOT NULL ORDER BY n DESC",
        )?;
        stmt.query_map([path], |r| {
            Ok(Version {
                id: r.get(0)?,
                at_ms: r.get(1)?,
                bytes: r.get(2)?,
                by: r.get(3)?,
            })
        })?
        .collect()
    }

    /// The files `bot`'s own `write` and `edit` changed (in `conversation`, when given), each
    /// once, by path: `true` when the first change made the file.
    pub fn changed_files(
        &self,
        bot: &str,
        conversation: Option<&str>,
    ) -> Result<Vec<(String, bool)>> {
        let mut stmt = self.0.prepare_cached(
            "SELECT path, earlier IS NULL FROM file_copies WHERE n IN (SELECT MIN(n) FROM file_copies \
             WHERE bot = ?1 AND who = 'bot' AND (?2 IS NULL OR conversation = ?2) GROUP BY path) \
             ORDER BY path",
        )?;
        stmt.query_map(params![bot, conversation], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect()
    }

    /// What the file `path` was before the first change of `bot`'s own `write` or `edit` (in
    /// `conversation`, when given): `Some(None)` when that change made it, `None` when no
    /// change is kept.
    pub fn first_copy(
        &self,
        bot: &str,
        conversation: Option<&str>,
        path: &str,
    ) -> Result<Option<Option<Vec<u8>>>> {
        self.0
            .query_row(
                "SELECT earlier FROM file_copies WHERE bot = ?1 AND who = 'bot' AND path = ?3 \
                 AND (?2 IS NULL OR conversation = ?2) ORDER BY n LIMIT 1",
                params![bot, conversation, path],
                |r| r.get(0),
            )
            .optional()
    }

    /// The newest version (one with content) of each file whose path starts with `dir/`, newest first.
    pub fn latest_under(&self, dir: &str) -> Result<Vec<(String, Version)>> {
        let mut stmt = self.0.prepare_cached(
            "SELECT path, id, at, LENGTH(earlier), who FROM file_copies \
             WHERE substr(path, 1, ?2) = ?1 AND earlier IS NOT NULL ORDER BY n DESC",
        )?;
        let prefix = format!("{dir}/");
        let rows = stmt.query_map(params![prefix, prefix.chars().count()], |r| {
            Ok((
                r.get::<_, String>(0)?,
                Version {
                    id: r.get(1)?,
                    at_ms: r.get(2)?,
                    bytes: r.get(3)?,
                    by: r.get(4)?,
                },
            ))
        })?;
        let mut seen = std::collections::HashSet::new();
        let mut latest = Vec::new();
        for row in rows {
            let (path, version) = row?;
            if seen.insert(path.clone()) {
                latest.push((path, version));
            }
        }
        Ok(latest)
    }

    /// The changes of the bot that the user undid and the bot has not been told of, oldest
    /// first: `(id, path, created)`, where `created` says the change made the file.
    pub fn untold(&self, bot: &str) -> Result<Vec<(String, String, bool)>> {
        let mut stmt = self.0.prepare_cached(
            "SELECT id, path, earlier IS NULL FROM file_copies \
             WHERE bot = ?1 AND who = 'bot' AND undone_at IS NOT NULL AND told = 0 \
             ORDER BY n LIMIT 20",
        )?;
        stmt.query_map([bot], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect()
    }

    pub fn set_told(&self, ids: &[String]) -> Result<()> {
        for id in ids {
            self.0
                .execute("UPDATE file_copies SET told = 1 WHERE id = ?1", [id])?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn a_snippet_is_cut_around_the_words_even_when_lower_case_is_longer() {
        // 'İ' lower-cases to two characters: places must still be the text's own.
        let text = format!("{}needle{}", "İ".repeat(50), "x".repeat(50));
        let shown = super::snippet(&text, "needle").unwrap();
        assert_eq!(
            shown,
            format!("…{}needle{}…", "İ".repeat(40), "x".repeat(40))
        );
        assert_eq!(super::snippet("Hello", "hello").unwrap(), "Hello");
        assert_eq!(super::snippet("Hello", "bye"), None);
    }

    use super::*;
    use proto::routines::RunOutcome;

    #[test]
    fn a_bots_copies_are_bounded_and_the_oldest_go_first_without_touching_another_bots() {
        let dir = std::env::temp_dir().join(format!("appd-store-copies-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::open(&dir.join("app.db")).unwrap();
        store.create_bot("b1", "Ada", 1).unwrap();
        store.create_bot("b2", "Bo", 1).unwrap();
        let copy = |id: &str, bytes: usize| Kept {
            id: id.into(),
            conversation: String::new(),
            path: "/f".into(),
            earlier: Some(vec![0; bytes]),
            after: None,
            who: "bot",
            at: 1,
            bot: None,
            undone: false,
        };
        store.keep(Owner::Bot("b2"), &copy("other", 1)).unwrap();
        // Fifty whole-file copies fill the bound: the entries' own weight tips it over.
        let file = proto::undo::MAX_FILE as usize;
        for n in 0..51 {
            store
                .keep(Owner::Bot("b1"), &copy(&format!("c{n}"), file))
                .unwrap();
        }
        let kept = |id: &str| store.kept(id).unwrap().is_some();
        assert!(!kept("c0") && !kept("c1"), "the oldest are dropped");
        assert!(kept("c2") && kept("c50"), "the newest are kept");
        assert!(kept("other"));
        let held: u64 = (0..51).filter(|n| kept(&format!("c{n}"))).count() as u64;
        assert!(held * (proto::undo::MAX_FILE + proto::undo::ENTRY) <= proto::undo::MAX_KEPT);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_routine_keeps_its_newest_runs_and_a_refusal_replaces_only_the_last() {
        let dir = std::env::temp_dir().join(format!("appd-store-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::open(&dir.join("app.db")).unwrap();
        store.create_bot("b1", "Ada", 1).unwrap();
        let run = |at_ms, outcome| RoutineRun {
            at_ms,
            outcome,
            error: None,
            reason: None,
            detail: None,
            by: None,
            tokens: None,
        };
        for at in 1..=MAX_RUNS as u64 + 5 {
            store
                .add_run("b1", "r1", &run(at, RunOutcome::Sent))
                .unwrap();
        }
        store
            .add_run("b1", "other", &run(1, RunOutcome::Sent))
            .unwrap();
        store
            .replace_last_run("b1", "r1", &run(999, RunOutcome::Failed))
            .unwrap();

        let runs = store.runs("b1", "r1").unwrap();
        assert_eq!(runs.len(), MAX_RUNS as usize);
        assert_eq!(runs[0], run(999, RunOutcome::Failed));
        assert_eq!(runs[1], run(MAX_RUNS as u64 + 4, RunOutcome::Sent));
        assert_eq!(runs.last().unwrap().at_ms, 6);
        assert_eq!(
            store.runs("b1", "other").unwrap(),
            [run(1, RunOutcome::Sent)]
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    fn project(id: &str, bots: &[&str], lead: Option<&str>) -> Project {
        serde_json::from_value(serde_json::json!({
            "id": id, "name": id, "bots": bots, "lead": lead,
        }))
        .unwrap()
    }

    fn count(store: &Store, table: &str, column: &str, id: &str) -> i64 {
        let sql = format!("SELECT count(*) FROM {table} WHERE {column} = ?1");
        store.0.query_row(&sql, [id], |r| r.get(0)).unwrap()
    }

    #[test]
    fn the_database_refuses_a_bot_in_two_projects_and_a_bot_or_lead_that_is_not_a_bot() {
        let dir = std::env::temp_dir().join(format!("appd-store-refuse-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::open(&dir.join("app.db")).unwrap();
        for id in ["a", "b"] {
            store.create_bot(id, id, 1).unwrap();
        }
        store
            .put_project(&project("p1", &["a", "b"], Some("a")))
            .unwrap();
        let kept = store.projects().unwrap();
        assert_eq!(
            (&kept[0].bots, &kept[0].lead),
            (&vec!["a".into(), "b".into()], &Some("a".into()))
        );
        assert_eq!(
            store.project_of("b").unwrap().unwrap().id.as_deref(),
            Some("p1")
        );

        // A bot of another project, one that is not a bot, a lead that is not one of the bots.
        assert!(store.put_project(&project("p2", &["b"], None)).is_err());
        assert!(store.put_project(&project("p2", &["ghost"], None)).is_err());
        assert!(
            store
                .put_project(&project("p2", &["a"], Some("ghost")))
                .is_err()
        );
        assert!(
            store
                .put_project(&project("p1", &["a"], Some("b")))
                .is_err()
        );
        assert_eq!(
            store.projects().unwrap(),
            kept,
            "a refused change leaves nothing behind"
        );

        // The tables themselves refuse too, whatever the code does.
        let db = &store.0;
        assert!(
            db.execute("UPDATE bots SET project = 'none' WHERE id = 'a'", [])
                .is_err()
        );
        assert!(
            db.execute("UPDATE projects SET lead = 'ghost'", [])
                .is_err()
        );
        assert!(
            db.execute("INSERT INTO ledger VALUES ('ghost', 'n', 'sent', 1)", [])
                .is_err()
        );
        assert!(db.execute("INSERT INTO mail(from_bot, to_bot, text, state, at) VALUES ('a', 'ghost', '', 'waiting', 1)", []).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn deleting_a_bot_or_a_project_leaves_no_row_that_points_at_it() {
        let dir = std::env::temp_dir().join(format!("appd-store-delete-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::open(&dir.join("app.db")).unwrap();
        let mut conversation = std::collections::HashMap::new();
        for id in ["a", "b"] {
            store.create_bot(id, id, 1).unwrap();
            let c = store.current_conversation(id).unwrap();
            store
                .append(&c, 1, &serde_json::json!({"type": "prompt"}))
                .unwrap();
            store.put_question(&c, "q", &serde_json::json!({})).unwrap();
            store.enqueue(&c, "n", "text", false).unwrap();
            conversation.insert(id, c);
            store.set_ledger(id, "n", Ledger::Sent, 1).unwrap();
            store.set_model(id, "m", None).unwrap();
            store.set_blocked(id, "web__x", true).unwrap();
            let used = proto::Tokens {
                input: 1,
                cached: 0,
                output: 1,
            };
            store
                .add_usage(id, "2026-01-01", &conversation[id], true, used)
                .unwrap();
            let limit = TokenLimit {
                day: Some(1),
                ..TokenLimit::default()
            };
            store.set_token_limit(id, &limit).unwrap();
            store
                .put_conversation_grant(
                    &conversation[id],
                    &Grant {
                        kind: Kind::Run,
                        target: "ls".into(),
                        at_ms: 1,
                    },
                )
                .unwrap();
            store
                .put_grant(
                    id,
                    &Grant {
                        kind: Kind::Connector,
                        target: "t".into(),
                        at_ms: 1,
                    },
                )
                .unwrap();
        }
        store.put_mail("a", "b", "hi", "waiting", 1, None).unwrap();
        store.put_mail("b", "a", "re", "waiting", 1, None).unwrap();
        store
            .put_project(&project("p1", &["a", "b"], Some("a")))
            .unwrap();
        store.put_room("p1", "a", "", "hello", 1).unwrap();

        store.delete_bot("a").unwrap();
        for table in BOT_TABLES {
            assert_eq!(count(&store, table, "bot", "a"), 0, "{table}");
        }
        assert_eq!(count(&store, "conversations", "bot", "a"), 0);
        // Made after version 1, so not among the tables a version 1 database moves.
        for table in CONVERSATION_TABLES
            .into_iter()
            .chain(["conversation_grants"])
        {
            assert_eq!(
                count(&store, table, "conversation", &conversation["a"]),
                0,
                "{table}"
            );
            assert_eq!(
                count(&store, table, "conversation", &conversation["b"]),
                1,
                "b keeps its own {table}"
            );
        }
        assert_eq!(count(&store, "bots", "id", "a"), 0);
        for column in ["from_bot", "to_bot"] {
            assert_eq!(count(&store, "mail", column, "a"), 0);
            assert_eq!(
                count(&store, "mail", column, "b"),
                0,
                "a message of a is gone for b too"
            );
        }
        let kept = store.projects().unwrap();
        assert_eq!((&kept[0].bots, &kept[0].lead), (&vec!["b".into()], &None));

        store.delete_project("p1").unwrap();
        assert_eq!(count(&store, "projects", "id", "p1"), 0);
        assert_eq!(count(&store, "room", "project", "p1"), 0);
        assert_eq!(count(&store, "bots", "project", "p1"), 0);
        assert_eq!(store.bot("b").unwrap().unwrap().id, "b");
        assert_eq!(store.project_of("b").unwrap(), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn an_old_database_is_not_moved_when_its_copy_cannot_be_written() {
        let dir = std::env::temp_dir().join(format!("appd-store-copy-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("app.db.before-1.part")).unwrap();
        let db = Connection::open(dir.join("app.db")).unwrap();
        db.execute_batch("CREATE TABLE bots(id TEXT PRIMARY KEY, name TEXT NOT NULL, created_at INTEGER NOT NULL, session TEXT);")
            .unwrap();
        drop(db);
        assert!(Store::open(&dir.join("app.db")).is_err());
        assert!(!dir.join("app.db.before-1").exists());
        let db = Connection::open(dir.join("app.db")).unwrap();
        let version: i64 = db
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, 0);
        assert!(!has_table(&db, "bots_old").unwrap() && !has_table(&db, "projects").unwrap());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_title_is_the_first_line_of_the_first_prompt_and_is_made_once() {
        assert_eq!(title_of("\n  Plan   the\tgarden \nmore"), "Plan the garden");
        let long = "é".repeat(TITLE_CHARS + 5);
        assert_eq!(title_of(&long), format!("{}…", "é".repeat(TITLE_CHARS)));

        let dir = std::env::temp_dir().join(format!("appd-store-title-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::open(&dir.join("app.db")).unwrap();
        store.create_bot("a", "a", 1).unwrap();
        let c = store.current_conversation("a").unwrap();
        assert!(
            !store.title_conversation(&c).unwrap(),
            "nothing to title yet"
        );
        for (nonce, text) in [("n1", "first"), ("n2", "second")] {
            let entry = serde_json::json!({"type": "prompt", "nonce": nonce, "text": text});
            store.append(&c, 1, &entry).unwrap();
        }
        assert!(store.title_conversation(&c).unwrap());
        assert!(!store.title_conversation(&c).unwrap(), "once");
        assert_eq!(store.conversation(&c).unwrap().unwrap().title, "first");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_fork_copies_the_conversation_up_to_the_prompt_and_the_last_used_one_is_current() {
        let dir = std::env::temp_dir().join(format!("appd-store-fork-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::open(&dir.join("app.db")).unwrap();
        store.create_bot("a", "a", 1).unwrap();
        let old = store.current_conversation("a").unwrap();
        for (nonce, text) in [("n1", "first"), ("n2", "second")] {
            let entry = serde_json::json!({"type": "prompt", "nonce": nonce, "text": text});
            store.append(&old, 2, &entry).unwrap();
            let end = serde_json::json!({"type": "turn_end", "reason": "done"});
            store.append(&old, 3, &end).unwrap();
        }
        let forked = store.fork_conversation(&old, "n2", 10).unwrap();
        assert_eq!((forked.bot.as_str(), forked.title.as_str()), ("a", "first"));
        let (copy, more) = store.history(&forked.id, i64::MAX, i64::MAX).unwrap();
        assert_eq!((copy.len(), more), (2, false));
        assert_eq!(store.history(&old, i64::MAX, i64::MAX).unwrap().0.len(), 4);
        assert_eq!(store.current_conversation("a").unwrap(), forked.id);

        // An archived conversation is not current while another is not; a prompt brings it back.
        store
            .set_conversation(&forked.id, None, None, Some(true))
            .unwrap();
        assert_eq!(store.current_conversation("a").unwrap(), old);
        store.touch_conversation(&forked.id, 20).unwrap();
        assert_eq!(store.current_conversation("a").unwrap(), forked.id);
        assert!(!store.conversation(&forked.id).unwrap().unwrap().archived);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_database_of_version_1_keeps_its_queue_questions_and_session_as_the_first_conversation() {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(
            "CREATE TABLE projects(id TEXT PRIMARY KEY, project TEXT NOT NULL, lead TEXT);
             CREATE TABLE bots(id TEXT PRIMARY KEY, name TEXT NOT NULL, created_at INTEGER NOT NULL, session TEXT, project TEXT);
             CREATE TABLE transcript(seq INTEGER PRIMARY KEY AUTOINCREMENT, bot TEXT NOT NULL, at INTEGER NOT NULL, entry TEXT NOT NULL);
             CREATE TABLE queue(seq INTEGER PRIMARY KEY, bot TEXT NOT NULL, nonce TEXT NOT NULL, text TEXT NOT NULL, UNIQUE(bot, nonce));
             CREATE TABLE questions(bot TEXT NOT NULL, question TEXT NOT NULL, entry TEXT NOT NULL, PRIMARY KEY(bot, question));
             INSERT INTO bots(id, name, created_at, session) VALUES ('a', 'a', 1, 'sess');
             INSERT INTO transcript(bot, at, entry) VALUES ('a', 5, '{\"type\":\"prompt\",\"nonce\":\"n\",\"text\":\"hi\"}');
             INSERT INTO queue VALUES (3, 'a', 'n2', 'later');
             INSERT INTO questions VALUES ('a', 'q', '{}');
             PRAGMA user_version = 1;",
        )
        .unwrap();
        migrate(&db).unwrap();
        let n = |sql: &str| -> i64 { db.query_row(sql, [], |r| r.get(0)).unwrap() };
        assert_eq!(n("SELECT count(*) FROM conversations"), 1);
        for table in CONVERSATION_TABLES {
            let sql = format!("SELECT count(*) FROM {table} WHERE conversation = 'a'");
            assert_eq!(n(&sql), 1, "{table}");
        }
        assert_eq!(
            n(
                "SELECT count(*) FROM conversations WHERE session = 'sess' AND touched_at = 5 AND title = 'hi'"
            ),
            1
        );
        assert_eq!(n("PRAGMA user_version"), SCHEMA_VERSION);
    }

    #[test]
    fn a_database_of_version_1_is_not_moved_when_its_copy_cannot_be_written() {
        let dir = std::env::temp_dir().join(format!("appd-store-copy2-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("app.db.before-2.part")).unwrap();
        let db = Connection::open(dir.join("app.db")).unwrap();
        db.execute_batch(
            "CREATE TABLE bots(id TEXT PRIMARY KEY, name TEXT NOT NULL, created_at INTEGER NOT NULL, session TEXT); PRAGMA user_version = 1;",
        )
        .unwrap();
        drop(db);
        assert!(Store::open(&dir.join("app.db")).is_err());
        assert!(!dir.join("app.db.before-2").exists());
        let db = Connection::open(dir.join("app.db")).unwrap();
        let version: i64 = db
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, 1);
        assert!(!has_table(&db, "conversations").unwrap());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_askers_of_a_message_follow_what_each_sender_was_working_on_and_an_old_mail_table_gets_the_link()
     {
        let dir = std::env::temp_dir().join(format!("appd-store-askers-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::open(&dir.join("app.db")).unwrap();
        for id in ["a", "b", "c"] {
            store.create_bot(id, id, 1).unwrap();
        }
        let first = store.put_mail("a", "b", "x", "answered", 1, None).unwrap();
        let second = store
            .put_mail("b", "c", "x", "waiting", 1, Some(first))
            .unwrap();
        let apart = store.put_mail("c", "a", "x", "waiting", 1, None).unwrap();
        assert_eq!(store.mail_askers(second).unwrap(), ["b", "a"]);
        assert_eq!(store.mail_askers(first).unwrap(), ["a"]);
        assert_eq!(store.mail_askers(apart).unwrap(), ["c"]);
        // A database of version 2 whose mail table has no `parent` yet gets it, and keeps its rows.
        store
            .0
            .execute_batch("ALTER TABLE mail DROP COLUMN parent")
            .unwrap();
        drop(store);
        let store = Store::open(&dir.join("app.db")).unwrap();
        assert_eq!(store.mail(first).unwrap().unwrap().from, "a");
        let third = store
            .put_mail("b", "c", "x", "waiting", 1, Some(first))
            .unwrap();
        assert_eq!(store.mail_askers(third).unwrap(), ["b", "a"]);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
