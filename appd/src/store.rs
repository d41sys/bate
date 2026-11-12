//! All state in one SQLite database, `app.db`, in write-ahead mode, written
//! only by the service. Each change is its own committed transaction, so a
//! row is on disk before the service acts on it.

use std::path::Path;

use proto::connectors::Connector;
use proto::routines::Routine;
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use serde_json::Value;

pub type Result<T, E = rusqlite::Error> = std::result::Result<T, E>;

pub struct Store(Connection);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BotRow {
    pub id: String,
    pub name: String,
    pub created_at: u64,
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

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS bots(id TEXT PRIMARY KEY, name TEXT NOT NULL, created_at INTEGER NOT NULL, session TEXT);
CREATE TABLE IF NOT EXISTS transcript(seq INTEGER PRIMARY KEY AUTOINCREMENT, bot TEXT NOT NULL, at INTEGER NOT NULL, entry TEXT NOT NULL);
CREATE INDEX IF NOT EXISTS transcript_bot ON transcript(bot, seq);
CREATE TABLE IF NOT EXISTS ledger(bot TEXT NOT NULL, nonce TEXT NOT NULL, state TEXT NOT NULL, at INTEGER NOT NULL, PRIMARY KEY(bot, nonce));
CREATE TABLE IF NOT EXISTS questions(bot TEXT NOT NULL, question TEXT NOT NULL, entry TEXT NOT NULL, PRIMARY KEY(bot, question));
CREATE TABLE IF NOT EXISTS routines(bot TEXT NOT NULL, id TEXT NOT NULL, routine TEXT NOT NULL, PRIMARY KEY(bot, id));
CREATE TABLE IF NOT EXISTS connectors(bot TEXT NOT NULL, name TEXT NOT NULL, connector TEXT NOT NULL, PRIMARY KEY(bot, name));
";

fn json<T: Serialize>(v: &T) -> String {
    serde_json::to_string(v).expect("serializable")
}

fn parse<T: serde::de::DeserializeOwned>(text: String) -> rusqlite::Result<T> {
    serde_json::from_str(&text).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
    })
}

impl Store {
    /// Opens (or creates) the database. Engines do not outlive the service, so
    /// a prompt still `sending` becomes uncertain and open questions are dropped.
    pub fn open(path: &Path) -> Result<Store> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        // FULL: a committed ledger row survives power loss, not only a crash.
        conn.pragma_update(None, "synchronous", "FULL")?;
        conn.execute_batch(SCHEMA)?;
        conn.execute(
            "UPDATE ledger SET state = 'uncertain' WHERE state = 'sending'",
            [],
        )?;
        conn.execute("DELETE FROM questions", [])?;
        Ok(Store(conn))
    }

    pub fn create_bot(&self, id: &str, name: &str, now: u64) -> Result<BotRow> {
        self.0.execute(
            "INSERT INTO bots(id, name, created_at) VALUES (?1, ?2, ?3)",
            params![id, name, now],
        )?;
        Ok(BotRow {
            id: id.to_owned(),
            name: name.to_owned(),
            created_at: now,
        })
    }

    pub fn bots(&self) -> Result<Vec<BotRow>> {
        let mut stmt = self
            .0
            .prepare_cached("SELECT id, name, created_at FROM bots ORDER BY created_at, id")?;
        stmt.query_map([], |r| {
            Ok(BotRow {
                id: r.get(0)?,
                name: r.get(1)?,
                created_at: r.get(2)?,
            })
        })?
        .collect()
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

    pub fn session(&self, bot: &str) -> Result<Option<String>> {
        self.0
            .query_row("SELECT session FROM bots WHERE id = ?1", [bot], |r| {
                r.get(0)
            })
    }

    pub fn set_session(&self, bot: &str, session: &str) -> Result<()> {
        self.0
            .execute(
                "UPDATE bots SET session = ?2 WHERE id = ?1",
                params![bot, session],
            )
            .map(drop)
    }

    pub fn clear_session(&self, bot: &str) -> Result<()> {
        self.0
            .execute("UPDATE bots SET session = NULL WHERE id = ?1", [bot])
            .map(drop)
    }

    /// Appends one transcript entry; returns its sequence number.
    pub fn append(&self, bot: &str, at: u64, entry: &Value) -> Result<i64> {
        self.0.execute(
            "INSERT INTO transcript(bot, at, entry) VALUES (?1, ?2, ?3)",
            params![bot, at, json(entry)],
        )?;
        Ok(self.0.last_insert_rowid())
    }

    /// A bot's whole transcript, oldest first, as `{seq, at, entry}`.
    // shortcut: no paging; #13 adds it for long chats.
    pub fn history(&self, bot: &str) -> Result<Vec<Value>> {
        let mut stmt = self
            .0
            .prepare_cached("SELECT seq, at, entry FROM transcript WHERE bot = ?1 ORDER BY seq")?;
        stmt.query_map([bot], |r| {
            let entry: Value = parse(r.get(2)?)?;
            Ok(serde_json::json!({"seq": r.get::<_, i64>(0)?, "at": r.get::<_, i64>(1)?, "entry": entry}))
        })?
        .collect()
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

    pub fn put_question(&self, bot: &str, question: &str, entry: &Value) -> Result<()> {
        self.0
            .execute(
                "INSERT OR REPLACE INTO questions(bot, question, entry) VALUES (?1, ?2, ?3)",
                params![bot, question, json(entry)],
            )
            .map(drop)
    }

    /// Removes an open question; `false` if it was not open.
    pub fn take_question(&self, bot: &str, question: &str) -> Result<bool> {
        Ok(self.0.execute(
            "DELETE FROM questions WHERE bot = ?1 AND question = ?2",
            [bot, question],
        )? > 0)
    }

    pub fn questions(&self, bot: &str) -> Result<Vec<Value>> {
        let mut stmt = self
            .0
            .prepare_cached("SELECT entry FROM questions WHERE bot = ?1 ORDER BY rowid")?;
        stmt.query_map([bot], |r| parse(r.get(0)?))?.collect()
    }

    pub fn clear_questions(&self, bot: &str) -> Result<()> {
        self.0
            .execute("DELETE FROM questions WHERE bot = ?1", [bot])
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

    pub fn delete_routine(&self, bot: &str, id: &str) -> Result<bool> {
        Ok(self
            .0
            .execute("DELETE FROM routines WHERE bot = ?1 AND id = ?2", [bot, id])?
            > 0)
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

    pub fn delete_connector(&self, bot: &str, name: &str) -> Result<bool> {
        Ok(self.0.execute(
            "DELETE FROM connectors WHERE bot = ?1 AND name = ?2",
            [bot, name],
        )? > 0)
    }
}
