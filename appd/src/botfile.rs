//! A bot in one file: its instructions and look, skills, routines, notes (when asked) and the
//! names and addresses of its connectors, as JSON with a format version. The file comes from
//! anywhere, so every part is checked by the rules that apply when the person enters it by
//! hand, before anything is made. The format is in `PROTOCOL.md`.

use crate::core::{
    check_avatar, check_bot_name, check_color, check_connector, check_connector_name,
    check_instructions, clean_routine,
};
use crate::memory::{self, MAX_NOTE, Place};
use crate::store::{BotRow, Profile, Store};
use crate::{download, paths, skills, sys};
use proto::connectors::{Connector, Transport};
use proto::routines::{Routine, Schedule};
use proto::skills::{self as skill_rules, DIR, FILE, Skill};
use ring::digest;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashSet;
use std::fs;
use std::io::Read;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

/// Most bytes of a file, checked before it is parsed.
const MAX_BYTES: u64 = 16 << 20;
const FORMAT: &str = "bate-bot";
const VERSION: u64 = 1;
const MOST_SKILLS: usize = 50;
const MOST_CONNECTORS: usize = 50;
const MOST_NOTES: usize = 500;
const MOST_ROUTINES: usize = 50;
/// A skill: as many files and folders and as deep as `skills::install` takes.
const MOST_ENTRIES: usize = 500;
const MOST_DEPTH: usize = 12;
/// Most bytes of a skill's `SKILL.md`, as the service reads it.
const MAX_SKILL_MD: usize = 64 * 1024;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BotFile {
    format: String,
    version: u64,
    name: String,
    #[serde(default)]
    instructions: String,
    #[serde(default)]
    avatar: String,
    #[serde(default)]
    color: String,
    #[serde(default)]
    skills: Vec<FileSkill>,
    #[serde(default)]
    routines: Vec<FileRoutine>,
    #[serde(default)]
    notes: Vec<FileText>,
    #[serde(default)]
    connectors: Vec<FileConnector>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileSkill {
    name: String,
    files: Vec<SkillFile>,
}

/// A note, or (`SkillFile`) a file of a skill: both are text.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileText {
    name: String,
    text: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SkillFile {
    /// Relative to the skill's folder, with `/` between parts.
    path: String,
    text: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct FileRoutine {
    name: String,
    prompt: String,
    #[serde(default)]
    schedule: Option<Schedule>,
    #[serde(default)]
    webhook: bool,
    #[serde(default)]
    chat: bool,
}

/// A connector without what signs it in: no headers, no environment, no tokens.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct FileConnector {
    name: String,
    address: Address,
    #[serde(default)]
    oauth: bool,
    #[serde(default)]
    client_id: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum Address {
    Stdio {
        command: String,
        #[serde(default)]
        args: Vec<String>,
    },
    Http {
        url: String,
    },
}

impl Address {
    fn text(&self) -> String {
        match self {
            Address::Http { url } => url.clone(),
            Address::Stdio { command, args } => std::iter::once(command)
                .chain(args)
                .cloned()
                .collect::<Vec<_>>()
                .join(" "),
        }
    }
}

/// A file that passed every check, as what would be made.
pub struct Checked {
    name: String,
    profile: Profile,
    skills: Vec<(Skill, Vec<SkillFile>)>,
    routines: Vec<Routine>,
    notes: Vec<FileText>,
    connectors: Vec<(Connector, String)>,
}

fn short(s: &str) -> String {
    let mut t: String = s.chars().take(40).collect();
    if s.chars().nth(40).is_some() {
        t.push('…');
    }
    t
}

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// The address without a user name, a query or a fragment, where a key could be.
fn plain_url(url: &str) -> String {
    let url = url.split(['?', '#']).next().unwrap_or_default();
    let Some((scheme, rest)) = url.split_once("://") else {
        return url.to_owned();
    };
    let (authority, path) = rest.split_once('/').map_or((rest, ""), |(a, p)| (a, p));
    let host = authority.rsplit('@').next().unwrap_or_default();
    if path.is_empty() {
        format!("{scheme}://{host}")
    } else {
        format!("{scheme}://{host}/{path}")
    }
}

/// Writes the bot's file to `exports/` in the service's home and gives its path. What the
/// import would refuse is refused here too.
pub fn export(store: &Store, bot: &str, notes: bool, now: u64) -> Result<String, String> {
    let row = store.bot(bot).map_err(err)?.ok_or("no such bot")?;
    let profile = store.profile(bot).map_err(err)?;
    let file = BotFile {
        format: FORMAT.into(),
        version: VERSION,
        name: row.name,
        instructions: profile.instructions,
        avatar: profile.avatar,
        color: profile.color,
        skills: skills_of(bot)?,
        routines: (store.routines(bot).map_err(err)?.into_iter())
            .map(|r| FileRoutine {
                name: r.name,
                prompt: r.prompt,
                schedule: r.schedule,
                webhook: r.webhook,
                chat: r.chat,
            })
            .collect(),
        notes: if notes { notes_of(bot)? } else { Vec::new() },
        connectors: (store.connectors(bot).map_err(err)?.into_iter())
            .map(|c| FileConnector {
                name: c.name,
                address: match c.transport {
                    Transport::Http { url, .. } => Address::Http {
                        url: plain_url(&url),
                    },
                    Transport::Stdio { command, args, .. } => Address::Stdio { command, args },
                },
                oauth: c.oauth,
                client_id: c.client_id,
            })
            .collect(),
    };
    let bytes = serde_json::to_vec_pretty(&file).map_err(err)?;
    // As the importing side reads it, so a file made here can always be imported.
    let value: Value = serde_json::from_slice(&bytes).map_err(err)?;
    check(serde_json::from_value(value).map_err(err)?, now)?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(too_big());
    }
    let dir = paths::home().join("exports");
    sys::create_private_dir(&dir).map_err(err)?;
    let path = dir.join(format!("bate-bot-{bot}-{now}.batebot"));
    let written = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)
        .and_then(|mut f| std::io::Write::write_all(&mut f, &bytes));
    written.map_err(|e| format!("the file was not written: {e}"))?;
    Ok(path.to_string_lossy().into_owned())
}

fn too_big() -> String {
    format!("a bot file may be at most {} MiB", MAX_BYTES >> 20)
}

/// The bot's skills with their files as text. A file that is not text, or a link, is not
/// carried: the export is refused and says which.
fn skills_of(bot: &str) -> Result<Vec<FileSkill>, String> {
    let Some(dir) = memory::folder(bot, DIR).map_err(err)? else {
        return Ok(Vec::new());
    };
    let mut found = Vec::new();
    // Bytes so far, over all skills.
    let mut bytes = 0;
    for skill in skills::list_in(&dir).map_err(err)? {
        let folder = dir.subdir(&skill.name, false).map_err(err)?;
        let mut files = Vec::new();
        collect(
            &folder,
            "",
            0,
            &mut files,
            &mut (0, &mut bytes),
            &skill.name,
        )?;
        found.push(FileSkill {
            name: skill.name,
            files,
        });
    }
    Ok(found)
}

fn collect(
    dir: &sys::Dir,
    prefix: &str,
    depth: usize,
    files: &mut Vec<SkillFile>,
    seen: &mut (usize, &mut u64),
    skill: &str,
) -> Result<(), String> {
    if depth > MOST_DEPTH {
        return Err(format!(
            "the skill `{skill}` is more than {MOST_DEPTH} folders deep"
        ));
    }
    let mut names = dir.names().map_err(err)?;
    names.sort();
    for name in names {
        seen.0 += 1;
        if seen.0 > MOST_ENTRIES {
            return Err(format!(
                "the skill `{skill}` has more than {MOST_ENTRIES} files and folders"
            ));
        }
        let path = format!("{prefix}{name}");
        if let Ok(folder) = dir.subdir(&name, false) {
            collect(&folder, &format!("{path}/"), depth + 1, files, seen, skill)?;
            continue;
        }
        let Some(file) = dir.regular(&name).map_err(err)? else {
            return Err(format!(
                "the skill `{skill}`: {path} is a link or a special file, so it cannot go in the file"
            ));
        };
        let mut bytes = Vec::new();
        let left = MAX_BYTES - *seen.1 + 1;
        file.take(left).read_to_end(&mut bytes).map_err(err)?;
        *seen.1 += bytes.len() as u64;
        if *seen.1 > MAX_BYTES {
            return Err(too_big());
        }
        let text = String::from_utf8(bytes).map_err(|_| {
            format!("the skill `{skill}`: {path} is not text, so it cannot go in the file")
        })?;
        files.push(SkillFile { path, text });
    }
    Ok(())
}

fn notes_of(bot: &str) -> Result<Vec<FileText>, String> {
    let place = Place::bot(bot);
    let mut notes = Vec::new();
    for note in memory::list(&place).map_err(err)? {
        if note.bytes > MAX_NOTE as u64 {
            return Err(format!(
                "the note {} is larger than {} KiB, so it cannot go in the file: shorten it, or leave the notes out",
                short(&note.name),
                MAX_NOTE >> 10
            ));
        }
        // Gone since it was listed: not there to carry.
        if let Some(text) = memory::read(&place, &note.name).map_err(err)? {
            notes.push(FileText {
                name: note.name,
                text,
            });
        }
    }
    Ok(notes)
}

/// Opens the file once, never through a link, and reads at most `MAX_BYTES` of it; these bytes
/// are what the preview and the import both work on.
fn read_bytes(path: &Path) -> Result<Vec<u8>, String> {
    let file = fs::OpenOptions::new()
        .read(true)
        // Not a link, and not a pipe that would wait.
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .map_err(|e| match e.raw_os_error() {
            Some(libc::ELOOP) => format!("{} is a link: choose the file itself", path.display()),
            _ => format!("cannot open {}: {e}", path.display()),
        })?;
    let meta = file.metadata().map_err(err)?;
    if !meta.is_file() {
        return Err(format!("{} is not a file", path.display()));
    }
    if meta.len() > MAX_BYTES {
        return Err(too_big());
    }
    let mut bytes = Vec::new();
    // One byte more than allowed: the file may have grown since it was measured.
    file.take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(too_big());
    }
    Ok(bytes)
}

/// The file at `path`, checked, and the SHA-256 of the bytes that were checked (hex).
pub fn read(path: &Path, now: u64) -> Result<(Checked, String), String> {
    let bytes = read_bytes(path)?;
    let sum = download::hex(digest::digest(&digest::SHA256, &bytes).as_ref());
    Ok((parse(&bytes, now)?, sum))
}

fn parse(bytes: &[u8], now: u64) -> Result<Checked, String> {
    let not_ours = || "this is not a Bate bot file".to_owned();
    let value: Value = serde_json::from_slice(bytes).map_err(|_| not_ours())?;
    if value.get("format").and_then(Value::as_str) != Some(FORMAT) {
        return Err(not_ours());
    }
    match value.get("version").and_then(Value::as_u64) {
        Some(VERSION) => {}
        Some(newer) if newer > VERSION => {
            return Err(format!(
                "this file is of format version {newer}; this Bate reads up to version {VERSION}. Update Bate to import it"
            ));
        }
        _ => return Err("the file's `version` is not one this Bate knows".into()),
    }
    let file =
        serde_json::from_value(value).map_err(|e| format!("the bot file is not valid: {e}"))?;
    check(file, now)
}

/// Every part by the rule that applies when it is entered by hand.
fn check(file: BotFile, now: u64) -> Result<Checked, String> {
    let name = file.name.trim().to_owned();
    check_bot_name(&name)?;
    check_instructions(&file.instructions)?;
    check_avatar(&file.avatar)?;
    check_color(&file.color)?;
    let profile = Profile {
        instructions: file.instructions,
        avatar: file.avatar,
        color: file.color,
    };

    let at_most = |what: &str, n: usize, most: usize| {
        if n > most {
            return Err(format!("a bot file may hold at most {most} {what}"));
        }
        Ok(())
    };
    at_most("skills", file.skills.len(), MOST_SKILLS)?;
    let mut skills = Vec::new();
    for s in file.skills {
        if skills.iter().any(|(k, _): &(Skill, _)| k.name == s.name) {
            return Err(format!(
                "the skill `{}` is in the file twice",
                short(&s.name)
            ));
        }
        skills.push(check_skill(s)?);
    }

    at_most("routines", file.routines.len(), MOST_ROUTINES)?;
    let mut routines = Vec::new();
    for r in file.routines {
        let mut routine = Routine {
            id: Some(sys::random_hex(4)),
            name: r.name,
            prompt: r.prompt,
            // A routine that arrives does nothing until the person turns it on.
            enabled: false,
            schedule: r.schedule,
            webhook: r.webhook,
            chat: r.chat,
            hook_token: r.webhook.then(|| sys::random_hex(16)),
            hook_url: None,
            anchor: None,
            next_run_ms: None,
            last_run: None,
        };
        let shown = short(&routine.name);
        let wrong = |e: String| format!("the routine `{shown}`: {e}");
        clean_routine(&mut routine).map_err(wrong)?;
        if let Some(s) = &routine.schedule {
            routine.anchor = Some(crate::schedule::anchor_today(s, now).map_err(wrong)?);
        }
        routines.push(routine);
    }

    at_most("notes", file.notes.len(), MOST_NOTES)?;
    let mut seen = HashSet::new();
    for n in &file.notes {
        let wrong = |e: &str| format!("the note `{}`: {e}", short(&n.name));
        memory::check_name(&n.name).map_err(|e| wrong(&e))?;
        if n.text.len() > MAX_NOTE {
            return Err(wrong(&format!("is larger than {} KiB", MAX_NOTE >> 10)));
        }
        // Names that differ in case are one file on a Mac.
        if !seen.insert(n.name.to_lowercase()) {
            return Err(wrong("is in the file twice"));
        }
    }

    at_most("connectors", file.connectors.len(), MOST_CONNECTORS)?;
    let mut connectors = Vec::new();
    for c in file.connectors {
        let wrong = |e: String| format!("the connector `{}`: {e}", short(&c.name));
        check_connector_name(&c.name).map_err(wrong)?;
        if connectors
            .iter()
            .any(|(k, _): &(Connector, String)| k.name == c.name)
        {
            return Err(wrong("is in the file twice".into()));
        }
        let address = c.address.text();
        let transport = match c.address {
            Address::Http { url } => Transport::Http {
                url,
                headers: Default::default(),
            },
            Address::Stdio { command, args } => Transport::Stdio {
                command,
                args,
                env: Default::default(),
            },
        };
        let connector = Connector {
            name: c.name.clone(),
            // To look at: off until the person has seen what it is and turns it on.
            enabled: false,
            transport,
            oauth: c.oauth,
            client_id: c.client_id,
            signed_in: None,
        };
        check_connector(&connector).map_err(wrong)?;
        connectors.push((connector, address));
    }

    Ok(Checked {
        name,
        profile,
        skills,
        routines,
        notes: file.notes,
        connectors,
    })
}

fn check_skill(s: FileSkill) -> Result<(Skill, Vec<SkillFile>), String> {
    let wrong = |e: String| format!("the skill `{}`: {e}", short(&s.name));
    skill_rules::check_name(&s.name).map_err(wrong)?;
    let mut files = HashSet::new();
    let mut folders = HashSet::new();
    for f in &s.files {
        // Each part is a plain name: nothing that goes up, starts at the root or is empty.
        let parts: Vec<&str> = f.path.split('/').collect();
        let plain = |p: &&str| {
            !(p.is_empty()
                || *p == "."
                || *p == ".."
                || p.len() > 255
                || p.chars().any(|c| c.is_control() || c == '\\'))
        };
        if !parts.iter().all(plain) {
            return Err(wrong(format!(
                "`{}` is not a path inside the skill's folder",
                short(&f.path)
            )));
        }
        if parts.len() > MOST_DEPTH + 1 {
            return Err(wrong(format!("{} is too deep", short(&f.path))));
        }
        if f.text.contains('\0') {
            return Err(wrong(format!("{} is not text", short(&f.path))));
        }
        // Names that differ in case are one file on a Mac.
        let key = f.path.to_lowercase();
        if !files.insert(key.clone()) {
            return Err(wrong(format!("{} is in the file twice", short(&f.path))));
        }
        for end in key.match_indices('/').map(|(i, _)| i) {
            folders.insert(key[..end].to_owned());
        }
    }
    if let Some(both) = files.intersection(&folders).next() {
        return Err(wrong(format!(
            "{} is both a file and a folder",
            short(both)
        )));
    }
    if files.len() + folders.len() > MOST_ENTRIES {
        return Err(wrong(format!(
            "it has more than {MOST_ENTRIES} files and folders"
        )));
    }
    let Some(main) = s.files.iter().find(|f| f.path == FILE) else {
        return Err(wrong(format!("it has no {FILE}")));
    };
    if main.text.len() > MAX_SKILL_MD {
        return Err(wrong(format!(
            "{FILE} is larger than {} KiB",
            MAX_SKILL_MD >> 10
        )));
    }
    let skill = skill_rules::parse(&main.text).map_err(wrong)?;
    if skill.name != s.name {
        return Err(wrong(format!("{FILE} names `{}`", short(&skill.name))));
    }
    Ok((skill, s.files))
}

/// What the file holds, for the person to see before anything is made.
pub fn preview(file: &Checked, sha256: &str) -> Value {
    json!({
        "name": file.name,
        "instructionsLength": file.profile.instructions.chars().count(),
        "skills": file.skills.iter().map(|(s, files)| json!({"name": s.name, "description": s.description, "files": files.len()})).collect::<Vec<_>>(),
        "routines": file.routines.iter().map(|r| json!({"name": r.name, "schedule": r.schedule, "webhook": r.webhook, "chat": r.chat})).collect::<Vec<_>>(),
        "notes": file.notes.len(),
        "connectors": file.connectors.iter().map(|(c, address)| json!({"name": c.name, "address": address})).collect::<Vec<_>>(),
        "sha256": sha256,
    })
}

/// The first of `name`, `name 2`, `name 3`… that no bot has, cut to fit the name's length.
fn free_name(name: &str, taken: &[String]) -> String {
    let free = |n: &str| !taken.contains(&n.to_lowercase());
    if free(name) {
        return name.to_owned();
    }
    (2..)
        .map(|n| {
            let suffix = format!(" {n}");
            let room = 80 - suffix.len();
            format!("{}{suffix}", name.chars().take(room).collect::<String>())
        })
        .find(|n| free(n))
        .expect("the numbers do not run out")
}

/// Makes the bot: its folder with notes and skills, then its rows in one transaction. If any
/// part fails, no row and no folder is left.
pub fn import(store: &Store, file: Checked, now: u64) -> Result<BotRow, String> {
    let taken: Vec<String> = (store.bots().map_err(err)?.into_iter())
        .map(|b| b.name.to_lowercase())
        .collect();
    let name = free_name(&file.name, &taken);
    // A fresh id: never one with a row or a folder, whose folder a failure would remove.
    let (id, dir) = loop {
        let id = sys::random_hex(4);
        let dir = paths::bot_dir(&id);
        if store.bot(&id).map_err(err)?.is_none() && fs::symlink_metadata(&dir).is_err() {
            break (id, dir);
        }
    };
    let made = (|| {
        sys::create_private_dir(&dir).map_err(|e| format!("cannot make the bot's folder: {e}"))?;
        let place = Place::bot(&id);
        for n in &file.notes {
            memory::write(&place, &n.name, n.text.as_bytes())
                .map_err(|e| format!("cannot write the note {}: {e}", short(&n.name)))?;
        }
        for (skill, files) in &file.skills {
            let files: Vec<(&str, &str)> = files.iter().map(|f| (&*f.path, &*f.text)).collect();
            skills::install_texts(&id, &skill.name, &files)?;
        }
        let connectors: Vec<Connector> = file.connectors.iter().map(|(c, _)| c.clone()).collect();
        store
            .create_bot_with(
                &id,
                &name,
                now,
                Some((&file.profile, &file.routines, &connectors)),
            )
            .map_err(err)
    })();
    if made.is_err() {
        // Ours alone: nothing has run in it.
        let _ = fs::remove_dir_all(&dir);
    }
    made
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_address_loses_what_could_be_a_key() {
        assert_eq!(
            plain_url("https://me:pw@host.example:8443/mcp/v1?key=abc#x"),
            "https://host.example:8443/mcp/v1"
        );
        assert_eq!(plain_url("http://127.0.0.1:9"), "http://127.0.0.1:9");
    }

    #[test]
    fn a_taken_name_gets_the_next_free_number_within_the_limit() {
        let taken = ["ada".to_owned(), "ada 2".to_owned()];
        assert_eq!(free_name("Ada", &taken), "Ada 3");
        assert_eq!(free_name("Bea", &taken), "Bea");
        let long = "x".repeat(80);
        let name = free_name(&long, std::slice::from_ref(&long));
        assert_eq!(name.chars().count(), 80);
        assert!(name.ends_with(" 2"));
    }
}
