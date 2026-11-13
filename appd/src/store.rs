//! All state in one SQLite database, `app.db`, in write-ahead mode, written
//! only by the service. Each change is its own committed transaction, so a
//! row is on disk before the service acts on it.

use std::path::Path;

use crate::oauth::Tokens;
use proto::connectors::Connector;
use proto::groups::Group;
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

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS bots(id TEXT PRIMARY KEY, name TEXT NOT NULL, created_at INTEGER NOT NULL, session TEXT);
CREATE TABLE IF NOT EXISTS transcript(seq INTEGER PRIMARY KEY AUTOINCREMENT, bot TEXT NOT NULL, at INTEGER NOT NULL, entry TEXT NOT NULL);
CREATE INDEX IF NOT EXISTS transcript_bot ON transcript(bot, seq);
CREATE TABLE IF NOT EXISTS ledger(bot TEXT NOT NULL, nonce TEXT NOT NULL, state TEXT NOT NULL, at INTEGER NOT NULL, PRIMARY KEY(bot, nonce));
CREATE TABLE IF NOT EXISTS questions(bot TEXT NOT NULL, question TEXT NOT NULL, entry TEXT NOT NULL, PRIMARY KEY(bot, question));
CREATE TABLE IF NOT EXISTS access(bot TEXT PRIMARY KEY, whole_disk INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS agents(bot TEXT PRIMARY KEY, command TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS permissions(bot TEXT NOT NULL, kind TEXT NOT NULL, level TEXT NOT NULL, PRIMARY KEY(bot, kind));
CREATE TABLE IF NOT EXISTS grants(bot TEXT NOT NULL, kind TEXT NOT NULL, target TEXT NOT NULL, at INTEGER NOT NULL, PRIMARY KEY(bot, kind, target));
CREATE TABLE IF NOT EXISTS routines(bot TEXT NOT NULL, id TEXT NOT NULL, routine TEXT NOT NULL, PRIMARY KEY(bot, id));
CREATE TABLE IF NOT EXISTS routine_runs(seq INTEGER PRIMARY KEY AUTOINCREMENT, bot TEXT NOT NULL, routine TEXT NOT NULL, run TEXT NOT NULL);
CREATE INDEX IF NOT EXISTS routine_runs_of ON routine_runs(bot, routine, seq);
CREATE TABLE IF NOT EXISTS oauth(bot TEXT NOT NULL, name TEXT NOT NULL, key TEXT NOT NULL UNIQUE, tokens TEXT, PRIMARY KEY(bot, name));
CREATE TABLE IF NOT EXISTS groups(id TEXT PRIMARY KEY, grp TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS connectors(bot TEXT NOT NULL, name TEXT NOT NULL, connector TEXT NOT NULL, PRIMARY KEY(bot, name));
CREATE TABLE IF NOT EXISTS profiles(bot TEXT PRIMARY KEY, instructions TEXT NOT NULL DEFAULT '', avatar TEXT NOT NULL DEFAULT '', color TEXT NOT NULL DEFAULT '', position INTEGER);
CREATE TABLE IF NOT EXISTS models(bot TEXT PRIMARY KEY, model TEXT NOT NULL, effort TEXT);
CREATE TABLE IF NOT EXISTS providers(id INTEGER PRIMARY KEY CHECK (id = 1), url TEXT NOT NULL, key TEXT);
";

/// Every table that holds rows of a bot, in its `bot` column.
const BOT_TABLES: [&str; 14] = [
    "transcript",
    "ledger",
    "questions",
    "access",
    "agents",
    "permissions",
    "grants",
    "routines",
    "routine_runs",
    "oauth",
    "connectors",
    "profiles",
    "models",
    "bots",
];

/// Past runs kept per routine.
const MAX_RUNS: u32 = 50;

/// A place `search` found the words.
#[derive(Debug, Serialize)]
pub struct Hit {
    pub bot: String,
    pub seq: i64,
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

    /// The model server URL and optional API key configured in the service.
    pub fn provider(&self) -> Result<Option<(String, Option<String>)>> {
        self.0
            .query_row("SELECT url, key FROM providers WHERE id = 1", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .optional()
    }

    pub fn set_provider(&self, url: &str, key: Option<&str>) -> Result<()> {
        self.0
            .execute(
                "INSERT INTO providers (id, url, key) VALUES (1, ?1, ?2)                  ON CONFLICT(id) DO UPDATE SET url = ?1, key = ?2",
                params![url, key],
            )
            .map(drop)
    }

    pub fn delete_provider(&self) -> Result<()> {
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

    /// Forgets the bot and everything stored for it.
    pub fn delete_bot(&mut self, bot: &str) -> Result<()> {
        let tx = self.0.transaction()?;
        for table in BOT_TABLES {
            let column = if table == "bots" { "id" } else { "bot" };
            tx.execute(&format!("DELETE FROM {table} WHERE {column} = ?1"), [bot])?;
        }
        tx.commit()
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

    /// The newest `limit` prompts and replies holding `needle` (any case), of `bot` or of all
    /// bots. A reply is searched whole, as the chat shows it: a run of text pieces, found at the
    /// `seq` of its first.
    // shortcut: reads every transcript entry per search; add an index (FTS5) when that gets slow.
    pub fn search(&self, bot: Option<&str>, needle: &str, limit: usize) -> Result<Vec<Hit>> {
        let mut stmt = self.0.prepare_cached(
            "SELECT seq, bot, entry FROM transcript WHERE ?1 IS NULL OR bot = ?1 ORDER BY bot, seq",
        )?;
        let mut rows = stmt.query([bot])?;
        let needle = needle.to_lowercase();
        let mut hits = Vec::new();
        // The item being read, as `chat.ts` makes them: (bot, seq, kind, text).
        let mut item: Option<(String, i64, &'static str, String)> = None;
        let done = |item: Option<(String, i64, &'static str, String)>, hits: &mut Vec<Hit>| {
            if let Some((bot, seq, kind, text)) = item
                && let Some(snippet) = snippet(&text, &needle)
            {
                hits.push(Hit {
                    bot,
                    seq,
                    kind,
                    snippet,
                });
            }
        };
        while let Some(row) = rows.next()? {
            let (seq, row_bot): (i64, String) = (row.get(0)?, row.get(1)?);
            let entry: Value = parse(row.get(2)?)?;
            let same_bot = item.as_ref().is_some_and(|(b, ..)| *b == row_bot);
            let text = |key: &str| entry[key].as_str().unwrap_or("").to_owned();
            let next = match entry["type"].as_str() {
                Some("prompt") => Some((seq, "user", text("text"))),
                Some("text") => {
                    if let Some((_, _, "reply", so_far)) = item.as_mut().filter(|_| same_bot) {
                        so_far.push_str(entry["delta"].as_str().unwrap_or(""));
                        continue;
                    }
                    Some((seq, "reply", text("delta")))
                }
                // Items the chat shows that are not searched; they end a reply.
                Some("thinking" | "tool_start") => Some((seq, "other", String::new())),
                Some("turn_end") if entry["reason"] != "done" => {
                    Some((seq, "other", String::new()))
                }
                _ => None,
            };
            if !same_bot {
                done(item.take(), &mut hits);
            }
            if let Some((seq, kind, text)) = next {
                done(item.replace((row_bot, seq, kind, text)), &mut hits);
            }
        }
        done(item, &mut hits);
        hits.sort_by_key(|h| std::cmp::Reverse(h.seq));
        hits.truncate(limit);
        Ok(hits)
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
    /// The last `limit` entries before `before`, oldest first, and whether there are older ones.
    pub fn history(&self, bot: &str, before: i64, limit: i64) -> Result<(Vec<Value>, bool)> {
        let mut stmt = self.0.prepare_cached(
            "SELECT seq, at, entry FROM transcript WHERE bot = ?1 AND seq < ?2 ORDER BY seq DESC LIMIT ?3",
        )?;
        // One more than asked for tells whether there is more.
        let mut page = stmt
            .query_map(rusqlite::params![bot, before, limit.saturating_add(1)], |r| {
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

    pub fn set_whole_disk(&self, bot: &str, whole_disk: bool) -> Result<()> {
        self.0
            .execute(
                "INSERT OR REPLACE INTO access(bot, whole_disk) VALUES (?1, ?2)",
                params![bot, whole_disk],
            )
            .map(drop)
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

    pub fn groups(&self) -> Result<Vec<Group>> {
        let mut stmt = self
            .0
            .prepare_cached("SELECT grp FROM groups ORDER BY id")?;
        stmt.query_map([], |r| parse(r.get(0)?))?.collect()
    }

    pub fn put_group(&self, group: &Group) -> Result<()> {
        let id = group.id.as_deref().expect("stored groups have an id");
        self.0
            .execute(
                "INSERT OR REPLACE INTO groups(id, grp) VALUES (?1, ?2)",
                params![id, json(group)],
            )
            .map(drop)
    }

    pub fn delete_group(&self, id: &str) -> Result<bool> {
        Ok(self.0.execute("DELETE FROM groups WHERE id = ?1", [id])? > 0)
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
        let text: Option<Option<String>> = self
            .0
            .query_row(
                "SELECT tokens FROM oauth WHERE bot = ?1 AND name = ?2",
                [bot, name],
                |r| r.get(0),
            )
            .optional()?;
        text.flatten().map(parse).transpose()
    }

    /// Keeps a sign-in, or forgets it (`None`). False if the connector has no key.
    pub fn set_tokens(&self, bot: &str, name: &str, tokens: Option<&Tokens>) -> Result<bool> {
        Ok(self.0.execute(
            "UPDATE oauth SET tokens = ?3 WHERE bot = ?1 AND name = ?2",
            params![bot, name, tokens.map(json)],
        )? > 0)
    }

    pub fn delete_oauth(&self, bot: &str, name: &str) -> Result<()> {
        self.0
            .execute(
                "DELETE FROM oauth WHERE bot = ?1 AND name = ?2",
                [bot, name],
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
