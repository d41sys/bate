//! The catalog: a list that this project keeps in a file of its repository
//! (`catalog/workplaces.json`) of workplaces that can be installed from a `.tar.gz` of their
//! folder, pinned by size and SHA-256 (format in `docs/workplaces.md`). Nothing here runs a
//! server: the list and the archives are read over HTTPS from where the entries say.

use crate::core::check_connector_name;
use crate::skills;
use crate::store::Workplace;
use crate::workplaces::{self, Download, Manifest, Refusal};
use crate::{download, sys};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::fs;
use std::path::Path;
use std::time::Duration;

/// Where the list is read; `BATE_CATALOG` names a stand-in.
const DEFAULT: &str = "https://raw.githubusercontent.com/d41sys/bate/main/catalog/workplaces.json";
const MAX_LIST: u64 = 1 << 20;
const MOST: usize = 200;
/// The most an archive may be: what a workplace's own files may come to.
const LARGEST: u64 = 200 << 20;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub name: String,
    pub description: String,
    pub version: String,
    pub url: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Deserialize)]
struct List {
    workplaces: Vec<Entry>,
}

fn failed(e: impl std::fmt::Display) -> Refusal {
    ("catalog_failed", e.to_string())
}

fn parse(text: &str) -> Result<Vec<Entry>, String> {
    let List { workplaces } = serde_json::from_str(text).map_err(|e| e.to_string())?;
    if workplaces.len() > MOST {
        return Err(format!("it has more than {MOST} entries"));
    }
    for (i, e) in workplaces.iter().enumerate() {
        let at = |what: String| format!("entry {} ({:?}): {what}", i + 1, e.name);
        check_connector_name(&e.name).map_err(&at)?;
        workplaces::line("version", &e.version, 40).map_err(&at)?;
        workplaces::line("description", &e.description, 500).map_err(&at)?;
        workplaces::line("url", &e.url, 2000).map_err(&at)?;
        workplaces::host_of(&e.url).map_err(|_| at("`url` must be an https address".into()))?;
        if e.size == 0 || e.size > LARGEST {
            return Err(at(format!("`size` is 1 to {LARGEST} bytes")));
        }
        if !workplaces::is_sha256(&e.sha256) {
            return Err(at("`sha256` is 64 lowercase hex digits".into()));
        }
        if workplaces[..i].iter().any(|o| o.name == e.name) {
            return Err(at("the name is used twice".into()));
        }
    }
    Ok(workplaces)
}

/// Reads the list.
pub fn list() -> Result<Vec<Entry>, Refusal> {
    let url = std::env::var("BATE_CATALOG")
        .ok()
        .filter(|u| !u.is_empty())
        .unwrap_or_else(|| DEFAULT.to_owned());
    let file = skills::staging().map_err(failed)?;
    let agent = download::agent(5, Duration::from_secs(60));
    let read = download::to_file(&agent, &url, MAX_LIST, &file, |_| {})
        .and_then(|_| fs::read_to_string(&file).map_err(|e| format!("cannot read it: {e}")))
        .and_then(|text| parse(&text).map_err(|e| format!("{url} is not a catalog: {e}")));
    skills::discard(Some(file));
    read.map_err(failed)
}

pub fn entry(name: &str) -> Result<Entry, Refusal> {
    (list()?.into_iter().find(|e| e.name == name)).ok_or_else(|| {
        (
            "no_such_catalog_entry",
            format!("the catalog has no `{name}`"),
        )
    })
}

/// Downloads the archive of `entry`, checks its size and hash against the catalog's, unpacks it
/// into a folder of its own and gives `then` that folder and its manifest, which must be the
/// entry's workplace and version. The archive and the folder are gone afterwards.
pub fn unpacked<T>(
    entry: &Entry,
    then: impl FnOnce(&Path, Manifest) -> Result<T, Refusal>,
) -> Result<T, Refusal> {
    let scratch = skills::staging().map_err(failed)?;
    let result = (|| {
        sys::create_private_dir(&scratch).map_err(failed)?;
        let (archive, folder) = (scratch.join("archive"), scratch.join("folder"));
        // The time allowed grows with the size, as for a workplace's downloads.
        let agent = download::agent(5, Duration::from_secs(600 + entry.size / (100 << 10)));
        let got =
            download::to_file(&agent, &entry.url, entry.size, &archive, |_| {}).map_err(failed)?;
        if got.size != entry.size {
            return Err(failed(format!(
                "{} is {} bytes, not the {} the catalog says",
                entry.url, got.size, entry.size
            )));
        }
        if got.sha256 != entry.sha256 {
            return Err(failed(format!(
                "the sha256 of {} is {}, not the catalog's {}",
                entry.url, got.sha256, entry.sha256
            )));
        }
        fs::create_dir(&folder).map_err(failed)?;
        workplaces::unpack(&archive, &folder).map_err(failed)?;
        let manifest = workplaces::preview(&folder)?;
        if manifest.name != entry.name || manifest.version != entry.version {
            return Err(failed(format!(
                "the archive holds {} {}, not the {} {} the catalog says",
                manifest.name, manifest.version, entry.name, entry.version
            )));
        }
        then(&folder, manifest)
    })();
    skills::discard(Some(scratch));
    result
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
        assert!(parse(r#"{"workplaces":[{"name":"a"}]}"#).is_err());
        let twice = format!(r#"{{"workplaces":[{one},{one}]}}"#);
        assert!(parse(&twice).unwrap_err().contains("twice"));
    }
}
