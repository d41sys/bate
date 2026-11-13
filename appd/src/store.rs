//! All state in one SQLite database, `app.db`, in write-ahead mode, written
//! only by the service. Each change is its own committed transaction, so a
//! row is on disk before the service acts on it.

use std::path::Path;

use proto::connectors::Connector;
use proto::permissions::{Grant, Kind, Level};
use proto::routines::{Routine, RoutineRun};
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
CREATE TABLE IF NOT EXISTS permissions(bot TEXT NOT NULL, kind TEXT NOT NULL, level TEXT NOT NULL, PRIMARY KEY(bot, kind));
CREATE TABLE IF NOT EXISTS grants(bot TEXT NOT NULL, kind TEXT NOT NULL, target TEXT NOT NULL, at INTEGER NOT NULL, PRIMARY KEY(bot, kind, target));
CREATE TABLE IF NOT EXISTS routines(bot TEXT NOT NULL, id TEXT NOT NULL, routine TEXT NOT NULL, PRIMARY KEY(bot, id));
CREATE TABLE IF NOT EXISTS routine_runs(seq INTEGER PRIMARY KEY AUTOINCREMENT, bot TEXT NOT NULL, routine TEXT NOT NULL, run TEXT NOT NULL);
CREATE INDEX IF NOT EXISTS routine_runs_of ON routine_runs(bot, routine, seq);
CREATE TABLE IF NOT EXISTS connectors(bot TEXT NOT NULL, name TEXT NOT NULL, connector TEXT NOT NULL, PRIMARY KEY(bot, name));
";

/// Past runs kept per routine.
const MAX_RUNS: u32 = 50;

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

    /// Removes the prompt entry with `nonce` and everything after it. Does nothing if there is none.
    /// The ledger keeps the removed prompts: a late retry of one must not undo the rewind.
    pub fn rewind_transcript(&self, bot: &str, nonce: &str) -> Result<()> {
        self.0
            .execute(
                "DELETE FROM transcript WHERE bot = ?1 AND seq >= (SELECT min(seq) FROM transcript
                 WHERE bot = ?1 AND json_extract(entry, '$.type') = 'prompt' AND json_extract(entry, '$.nonce') = ?2)",
                [bot, nonce],
            )
            .map(drop)
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

    /// Removes the grants for the tools of connector `name`; returns how many.
    pub fn delete_connector_grants(&self, bot: &str, name: &str) -> Result<usize> {
        let prefix = format!("{name}__");
        self.0.execute(
            "DELETE FROM grants WHERE bot = ?1 AND kind = ?2 AND substr(target, 1, length(?3)) = ?3",
            params![bot, json(&Kind::Connector), prefix],
        )
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

#[cfg(test)]
mod tests {
    use super::*;
    use proto::routines::RunOutcome;

    #[test]
    fn a_routine_keeps_its_newest_runs_and_a_refusal_replaces_only_the_last() {
        let dir = std::env::temp_dir().join(format!("appd-store-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::open(&dir.join("app.db")).unwrap();
        let run = |at_ms, outcome| RoutineRun {
            at_ms,
            outcome,
            error: None,
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
}
