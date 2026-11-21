//! The catalog: three lists that this project keeps in files of its repository (`catalog/`).
//! `workplaces.json` lists workplaces that can be installed from a `.tar.gz` of their folder,
//! pinned by size and SHA-256 (format in `docs/workplaces.md`); `skills.json` lists skills the
//! same way; `connectors.json` lists connectors as the values of the "New connector" form, which
//! installs nothing. An entry is a pointer, not something Bate has checked. Nothing here runs a
//! server: a list and an archive are read over HTTPS from where the entries say.

use crate::core::{check_connector, check_connector_name};
use crate::skills;
use crate::store::Workplace;
use crate::workplaces::{self, Download, Manifest, Refusal};
use crate::{download, sys};
use proto::connectors::{Connector, Transport};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::fs;
use std::path::Path;
use std::time::Duration;

/// The folder the lists are read from; `BATE_CATALOG` names a stand-in folder.
const DEFAULT: &str = "https://raw.githubusercontent.com/d41sys/bate/main/catalog/";
const MAX_LIST: u64 = 1 << 20;
const MOST: usize = 200;
/// The most an archive may be: what a workplace's own files may come to.
const LARGEST: u64 = 200 << 20;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub name: String,
    pub description: String,
    /// Who says they made it: a claim in the list, which nothing checks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    pub version: String,
    pub url: String,
    pub size: u64,
    pub sha256: String,
}

fn failed(e: impl std::fmt::Display) -> Refusal {
    ("catalog_failed", e.to_string())
}

/// `Ok` when `description` and `author` are lines as a list may carry them.
fn check_about(description: &str, author: &Option<String>) -> Result<(), String> {
    workplaces::line("description", description, 500)?;
    author
        .as_deref()
        .map_or(Ok(()), proto::skills::check_author)
}

/// The archive an entry pins: an https address, a size and a hash.
fn check_pin(url: &str, size: u64, sha256: &str) -> Result<(), String> {
    workplaces::line("url", url, 2000)?;
    workplaces::host_of(url).map_err(|_| "`url` must be an https address".to_owned())?;
    if size == 0 || size > LARGEST {
        return Err(format!("`size` is 1 to {LARGEST} bytes"));
    }
    if !workplaces::is_sha256(sha256) {
        return Err("`sha256` is 64 lowercase hex digits".into());
    }
    Ok(())
}

/// The entries under `key` in `text`, each accepted by `check` and none with a name twice.
fn parse_list<T: DeserializeOwned>(
    text: &str,
    key: &str,
    name: impl Fn(&T) -> &str,
    check: impl Fn(&T) -> Result<(), String>,
) -> Result<Vec<T>, String> {
    let mut list: serde_json::Map<String, Value> =
        serde_json::from_str(text).map_err(|e| e.to_string())?;
    let entries = list.remove(key).ok_or(format!("it has no `{key}`"))?;
    let entries: Vec<T> = serde_json::from_value(entries).map_err(|e| e.to_string())?;
    if entries.len() > MOST {
        return Err(format!("it has more than {MOST} entries"));
    }
    for (i, e) in entries.iter().enumerate() {
        let at = |what: String| format!("entry {} ({:?}): {what}", i + 1, name(e));
        check(e).map_err(&at)?;
        if entries[..i].iter().any(|o| name(o) == name(e)) {
            return Err(at("the name is used twice".into()));
        }
    }
    Ok(entries)
}

fn parse(text: &str) -> Result<Vec<Entry>, String> {
    parse_list(
        text,
        "workplaces",
        |e: &Entry| &e.name,
        |e| {
            check_connector_name(&e.name)?;
            workplaces::line("version", &e.version, 40)?;
            check_about(&e.description, &e.author)?;
            check_pin(&e.url, e.size, &e.sha256)
        },
    )
}

/// A skill of the list: its folder, packed as a `.tar.gz` with `SKILL.md` at the top.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillEntry {
    pub name: String,
    pub description: String,
    /// Who says they made it: a claim in the list, which nothing checks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    pub url: String,
    pub size: u64,
    pub sha256: String,
}

fn parse_skills(text: &str) -> Result<Vec<SkillEntry>, String> {
    parse_list(
        text,
        "skills",
        |e: &SkillEntry| &e.name,
        |e| {
            proto::skills::check_name(&e.name)?;
            check_about(&e.description, &e.author)?;
            check_pin(&e.url, e.size, &e.sha256)
        },
    )
}

/// A connector of the list: what the "New connector" form is filled in with. Never a value of
/// the person's own: `fill` names what they must add (a key, a folder).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectorEntry {
    pub name: String,
    pub description: String,
    /// Who says they made it: a claim in the list, which nothing checks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    /// The page the address or command was read from: the maker's README or docs.
    pub home: String,
    pub transport: Transport,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub oauth: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fill: Vec<String>,
}

impl ConnectorEntry {
    /// The connector the form would save as it is: checked as `set_connector` checks.
    fn connector(&self) -> Connector {
        Connector {
            name: self.name.clone(),
            enabled: true,
            transport: self.transport.clone(),
            oauth: self.oauth,
            client_id: None,
            signed_in: None,
        }
    }
}

fn parse_connectors(text: &str) -> Result<Vec<ConnectorEntry>, String> {
    parse_list(
        text,
        "connectors",
        |e: &ConnectorEntry| &e.name,
        |e| {
            check_connector(&e.connector())?;
            check_about(&e.description, &e.author)?;
            if !e.home.starts_with("https://") {
                return Err("`home` must be an https address".into());
            }
            workplaces::line("home", &e.home, 2000)?;
            let left_out = |what: &str| {
                format!("`{what}` is left out: name what is to be filled in under `fill`")
            };
            match &e.transport {
                Transport::Stdio { env, .. } if !env.is_empty() => Err(left_out("env")),
                Transport::Http { headers, .. } if !headers.is_empty() => Err(left_out("headers")),
                _ => Ok(()),
            }?;
            if e.fill.len() > 5 {
                return Err("`fill` has at most 5 entries".into());
            }
            e.fill
                .iter()
                .try_for_each(|f| workplaces::line("fill", f, 80))
        },
    )
}

/// Reads the list `file` of the catalog folder, and `parse`s it.
fn read<T>(
    file: &str,
    parse: impl FnOnce(&str) -> Result<Vec<T>, String>,
) -> Result<Vec<T>, Refusal> {
    let base = std::env::var("BATE_CATALOG")
        .ok()
        .filter(|u| !u.is_empty())
        .unwrap_or_else(|| DEFAULT.to_owned());
    let url = format!("{base}{file}");
    let staged = skills::staging().map_err(failed)?;
    let agent = download::agent(5, Duration::from_secs(60));
    let read = download::to_file(&agent, &url, MAX_LIST, &staged, |_| {})
        .and_then(|_| fs::read_to_string(&staged).map_err(|e| format!("cannot read it: {e}")))
        .and_then(|text| parse(&text).map_err(|e| format!("{url} is not a catalog: {e}")));
    skills::discard(Some(staged));
    read.map_err(failed)
}

/// Reads the list of workplaces.
pub fn list() -> Result<Vec<Entry>, Refusal> {
    read("workplaces.json", parse)
}

/// Reads the list of connectors.
pub fn connector_list() -> Result<Vec<ConnectorEntry>, Refusal> {
    read("connectors.json", parse_connectors)
}

/// Reads the list of skills.
pub fn skill_list() -> Result<Vec<SkillEntry>, Refusal> {
    read("skills.json", parse_skills)
}

fn no_entry(name: &str) -> Refusal {
    (
        "no_such_catalog_entry",
        format!("the catalog has no `{name}`"),
    )
}

pub fn entry(name: &str) -> Result<Entry, Refusal> {
    (list()?.into_iter().find(|e| e.name == name)).ok_or_else(|| no_entry(name))
}

pub fn skill_entry(name: &str) -> Result<SkillEntry, Refusal> {
    (skill_list()?.into_iter().find(|e| e.name == name)).ok_or_else(|| no_entry(name))
}

/// Downloads the archive at `url`, checks its size and hash against the ones the catalog pins,
/// unpacks it into a folder of its own and gives `then` that folder. The archive and the folder
/// are gone afterwards.
fn pinned<T>(
    (url, size, sha256): (&str, u64, &str),
    then: impl FnOnce(&Path) -> Result<T, Refusal>,
) -> Result<T, Refusal> {
    let scratch = skills::staging().map_err(failed)?;
    let result = (|| {
        sys::create_private_dir(&scratch).map_err(failed)?;
        let (archive, folder) = (scratch.join("archive"), scratch.join("folder"));
        // The time allowed grows with the size, as for a workplace's downloads.
        let agent = download::agent(5, Duration::from_secs(600 + size / (100 << 10)));
        let got = download::to_file(&agent, url, size, &archive, |_| {}).map_err(failed)?;
        if got.size != size {
            return Err(failed(format!(
                "{url} is {} bytes, not the {size} the catalog says",
                got.size
            )));
        }
        if got.sha256 != sha256 {
            return Err(failed(format!(
                "the sha256 of {url} is {}, not the catalog's {sha256}",
                got.sha256
            )));
        }
        fs::create_dir(&folder).map_err(failed)?;
        workplaces::unpack("tar.gz", &archive, &folder).map_err(failed)?;
        then(&folder)
    })();
    skills::discard(Some(scratch));
    result
}

/// The folder of `entry`'s archive, whose manifest must be the entry's workplace and version.
pub fn unpacked<T>(
    entry: &Entry,
    then: impl FnOnce(&Path, Manifest) -> Result<T, Refusal>,
) -> Result<T, Refusal> {
    pinned((&entry.url, entry.size, &entry.sha256), |folder| {
        let manifest = workplaces::preview(folder)?;
        if manifest.name != entry.name || manifest.version != entry.version {
            return Err(failed(format!(
                "the archive holds {} {}, not the {} {} the catalog says",
                manifest.name, manifest.version, entry.name, entry.version
            )));
        }
        then(folder, manifest)
    })
}

/// What `then` makes of the folder of `entry`'s archive, whose skill must be the entry's.
pub fn skill_unpacked<T>(
    entry: &SkillEntry,
    then: impl FnOnce(&Path) -> Result<T, String>,
) -> Result<T, Refusal> {
    pinned((&entry.url, entry.size, &entry.sha256), |folder| {
        let invalid = |e: String| ("invalid_skill", e);
        let (skill, ..) = skills::preview(folder).map_err(invalid)?;
        if skill.name != entry.name {
            return Err(failed(format!(
                "the archive holds the skill {}, not the {} the catalog says",
                skill.name, entry.name
            )));
        }
        then(folder).map_err(invalid)
    })
}

/// What a workplace asks for that the installed one did not.
#[derive(Debug, Serialize)]
pub struct Newly {
    pub folders: Vec<String>,
    pub hosts: Vec<String>,
    pub commands: Vec<String>,
    pub downloads: Vec<Download>,
}

/// What `new` needs beyond `old`: all of it when nothing is installed.
pub fn newly(new: &Manifest, old: Option<&Manifest>) -> Newly {
    let added = |new: &[String], old: &[String]| -> Vec<String> {
        new.iter().filter(|x| !old.contains(x)).cloned().collect()
    };
    let no_needs = workplaces::Needs::default();
    let (needs, downloads) = old.map_or((&no_needs, &[][..]), |m| (&m.needs, &m.downloads[..]));
    Newly {
        folders: added(&new.needs.folders, &needs.folders),
        hosts: added(&new.needs.hosts, &needs.hosts),
        commands: added(&new.needs.commands, &needs.commands),
        downloads: (new.downloads.iter())
            .filter(|d| !downloads.contains(d))
            .cloned()
            .collect(),
    }
}

/// An entry with what is installed of it. `update`: what is installed is not the archive the
/// entry pins (a workplace installed from a folder has no pin, so the entry is offered).
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Item<'a> {
    #[serde(flatten)]
    entry: &'a Entry,
    installed_version: Option<&'a str>,
    update: bool,
}

pub fn listed(entries: &[Entry], installed: &[Workplace]) -> Value {
    let items: Vec<_> = (entries.iter())
        .map(|entry| {
            let row = installed.iter().find(|w| w.name == entry.name);
            Item {
                entry,
                installed_version: row.map(|w| w.version.as_str()),
                update: row.is_some_and(|w| w.archive.as_deref() != Some(&entry.sha256)),
            }
        })
        .collect();
    json!(items)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_list_is_checked() {
        let sha = "a".repeat(64);
        let one = format!(
            r#"{{"name":"counter","description":"d","version":"1","url":"https://example.com/c.tar.gz","size":5,"sha256":"{sha}"}}"#
        );
        let entry = || format!(r#"{{"workplaces":[{one}]}}"#);
        assert_eq!(parse(&entry()).unwrap().len(), 1);
        let bad = |from: &str, to: &str| parse(&entry().replace(from, to)).is_err();
        assert!(bad("counter", "Bad Name"));
        assert!(bad("https://example.com", "http://example.com"));
        assert!(bad("\"size\":5", "\"size\":0"));
        assert!(bad(&sha, "ABC"));
        assert!(bad("\"d\"", "\"\""));
        let author = |a: &str| entry().replace("\"d\"", &format!("\"d\",\"author\":{a}"));
        assert_eq!(
            parse(&author("\"Ada L.\"")).unwrap()[0].author.as_deref(),
            Some("Ada L.")
        );
        for bad in ["\"\"", "\"a\\nb\"", &format!("\"{}\"", "x".repeat(101))] {
            assert!(parse(&author(bad)).is_err(), "{bad}");
        }
        assert!(parse(r#"{"workplaces":[{"name":"a"}]}"#).is_err());
        let twice = format!(r#"{{"workplaces":[{one},{one}]}}"#);
        assert!(parse(&twice).unwrap_err().contains("twice"));
    }

    const ONE: &str = r#"{"name":"notes","description":"d","home":"https://example.com/docs","transport":{"type":"http","url":"https://example.com/mcp"},"oauth":true}"#;

    #[test]
    fn a_connector_list_is_checked_as_the_form_is() {
        let list = |one: &str| format!(r#"{{"connectors":[{one}]}}"#);
        assert_eq!(parse_connectors(&list(ONE)).unwrap().len(), 1);
        let bad = |from: &str, to: &str| parse_connectors(&list(&ONE.replace(from, to))).is_err();
        assert!(bad("notes", "Bad Name"));
        assert!(bad("https://example.com/mcp", "ftp://example.com/mcp"));
        assert!(bad("https://example.com/docs", "http://example.com/docs"));
        // Sign-in is for a connector reached over http.
        assert!(bad(
            r#""transport":{"type":"http","url":"https://example.com/mcp"}"#,
            r#""transport":{"type":"stdio","command":"x"}"#
        ));
        assert!(bad(
            r#""type":"http","url""#,
            r#""type":"http","headers":{"a":"b"},"url""#
        ));
        assert!(bad(
            r#""transport":{"type":"http","url":"https://example.com/mcp"},"oauth":true"#,
            r#""transport":{"type":"stdio","command":"x","env":{"KEY":"v"}}"#
        ));
        assert!(bad(
            r#""oauth":true"#,
            r#""fill":["a","b","c","d","e","f"]"#
        ));
        assert!(bad(r#""oauth":true"#, r#""fill":[""]"#));
        assert!(
            parse_connectors(&list(&format!("{ONE},{ONE}")))
                .unwrap_err()
                .contains("twice")
        );
        assert!(parse_connectors(r#"{"workplaces":[]}"#).is_err());
    }

    #[test]
    fn the_lists_of_the_repository_are_good() {
        let connectors = parse_connectors(include_str!("../../catalog/connectors.json")).unwrap();
        assert!(
            connectors.len() <= 10,
            "the list keeps to at most 10 connectors"
        );
        assert!(connectors.iter().all(|c| c.author.is_some()));
        parse_skills(include_str!("../../catalog/skills.json")).unwrap();
        parse(include_str!("../../catalog/workplaces.json")).unwrap();
    }
}
