//! Adapters of installed agents that Bate fetches itself. An adapter is a Node program (an npm
//! package and every package it runs on). The list (`catalog/agents.json`) pins each tarball by
//! size and SHA-256; this module downloads those, unpacks them under `agents/<name>/node_modules`
//! and writes a one-line script `agents/.bin/<name>` that starts the adapter with the `node` the
//! person already has. No package manager and no install script is ever run.
//!
//! `agents/.bin` is the last folder of the agent `PATH` (`login_path`), so a program of the same name
//! that the person put on their own `PATH` is found first, is used, and is never touched.

use crate::catalog::{self, AdapterEntry, Package};
use crate::workplaces::Refusal;
use crate::{login_path, paths, project_server, skills, sys, workplaces};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashSet;
use std::fs;
use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Marks a finished install: what is in the folder, written last.
const MARK: &str = "adapter.json";
/// Install scripts: what a package manager would run and this never does.
const SCRIPTS: [&str; 3] = ["preinstall", "install", "postinstall"];

fn failed(e: impl std::fmt::Display) -> Refusal {
    ("adapter_failed", e.to_string())
}

/// Where the adapter `name` is kept.
pub fn folder(name: &str) -> PathBuf {
    paths::home().join("agents").join(name)
}

/// The folder of the scripts that start the adapters.
pub fn bin_dir() -> PathBuf {
    paths::home().join("agents/.bin")
}

/// The version of Bate's own copy of `name`, if one is installed.
pub fn installed(name: &str) -> Option<String> {
    #[derive(Deserialize)]
    struct Mark {
        version: String,
    }
    let text = fs::read_to_string(folder(name).join(MARK)).ok()?;
    serde_json::from_str::<Mark>(&text).ok().map(|m| m.version)
}

/// This Mac's processor as the list names it.
fn cpu() -> &'static str {
    match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86_64" => "x64",
        other => other,
    }
}

/// The packages of `entry` that this Mac gets.
fn wanted(entry: &AdapterEntry) -> Vec<&Package> {
    let cpu = cpu();
    (entry.packages.iter())
        .filter(|p| p.cpu.as_deref().is_none_or(|c| c == cpu))
        .collect()
}

/// The name the other packages ask for the one in `path` by.
fn folder_name(path: &str) -> &str {
    path.rsplit_once("node_modules/").map_or(path, |(_, n)| n)
}

/// The `node` the adapter runs on: where it is on the agent `PATH`, and its version, or what is
/// missing. Waits for the login shell the first time, and runs `node`: not for the core thread.
fn runtime(least: u32) -> Result<(PathBuf, String), String> {
    let path = login_path::get();
    let node = project_server::on_path_in(path, "node").ok_or(
        "Node.js is not installed: there is no `node` on the PATH Bate starts agents with",
    )?;
    let child = {
        let _spawning = sys::SPAWN.lock().unwrap_or_else(|e| e.into_inner());
        Command::new(&node)
            .arg("--version")
            .env("PATH", path)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("cannot run {}: {e}", node.display()))?
    };
    let out = child
        .wait_with_output()
        .map_err(|e| format!("cannot run {}: {e}", node.display()))?;
    let said = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    let major = said
        .strip_prefix('v')
        .and_then(|v| v.split('.').next()?.parse::<u32>().ok());
    match major {
        None => Err(format!(
            "{} did not say which version of Node it is",
            node.display()
        )),
        Some(m) if m < least => Err(format!(
            "Node.js {least} or newer is needed, and {} is {said}",
            node.display()
        )),
        Some(_) => Ok((node, said)),
    }
}

/// What installing `entry` would do, to show before anything is fetched: the listed packages for
/// this Mac, their size and hosts, the `node` it would run on (or what is missing: then the
/// install is refused). Nothing is contacted or kept.
pub fn preview(entry: &AdapterEntry) -> Value {
    let packages = wanted(entry);
    let mut hosts: Vec<String> = (packages.iter())
        .filter_map(|p| workplaces::host_of(&p.url).ok())
        .collect();
    hosts.sort();
    hosts.dedup();
    let (runtime, missing) = match runtime(entry.node) {
        Ok((path, version)) => (json!({"path": path, "version": version}), Value::Null),
        Err(why) => (Value::Null, json!(why)),
    };
    let have = installed(&entry.name);
    json!({
        "name": entry.name,
        "description": entry.description,
        "version": entry.version,
        "home": entry.home,
        "installedVersion": have,
        "update": have.as_ref().is_some_and(|v| *v != entry.version),
        "packages": packages.len(),
        "bytes": packages.iter().map(|p| p.size).sum::<u64>(),
        "hosts": hosts,
        "node": entry.node,
        "runtime": runtime,
        "missing": missing,
    })
}

/// A package's own description of itself, as far as the install looks at it.
#[derive(Deserialize)]
struct Described {
    name: String,
    version: String,
    #[serde(default)]
    scripts: std::collections::HashMap<String, Value>,
    #[serde(default)]
    dependencies: std::collections::HashMap<String, Value>,
    #[serde(default, rename = "peerDependencies")]
    peers: std::collections::HashMap<String, Value>,
    #[serde(default, rename = "peerDependenciesMeta")]
    peers_meta: std::collections::HashMap<String, Value>,
}

/// Checks the unpacked package in `at` against the list's pin: it is the one named, it has no
/// install script, and everything it needs at run time is another pinned package (`names`).
/// Optional dependencies are left out, as an install without them works.
fn check_package(pin: &Package, at: &Path, names: &HashSet<&str>) -> Result<(), Refusal> {
    let text = fs::read_to_string(at.join("package.json"))
        .map_err(|e| failed(format!("{} has no readable package.json: {e}", pin.name)))?;
    let pj: Described = serde_json::from_str(&text)
        .map_err(|e| failed(format!("the package.json of {} is not one: {e}", pin.name)))?;
    if pj.name != pin.name || pj.version != pin.version {
        return Err(failed(format!(
            "the archive holds {} {}, not the {} {} the list says",
            pj.name, pj.version, pin.name, pin.version
        )));
    }
    if let Some(script) = SCRIPTS.iter().find(|s| pj.scripts.contains_key(**s)) {
        return Err(failed(format!(
            "{} {} runs a `{script}` script when installed; Bate runs none, so it is not installed",
            pin.name, pin.version
        )));
    }
    if at.join("binding.gyp").exists() {
        return Err(failed(format!(
            "{} {} must be built when installed; Bate builds nothing, so it is not installed",
            pin.name, pin.version
        )));
    }
    let required_peers = (pj.peers.keys()).filter(|p| {
        let optional = pj.peers_meta.get(*p).and_then(|m| m.get("optional"));
        optional != Some(&Value::Bool(true))
    });
    for dependency in pj.dependencies.keys().chain(required_peers) {
        if !names.contains(dependency.as_str()) {
            return Err(failed(format!(
                "{} {} needs {dependency}, which the list does not pin",
                pin.name, pin.version
            )));
        }
    }
    Ok(())
}

/// A finished copy in the staging folder, not yet in place.
pub struct Built {
    pub name: String,
    pub version: String,
    tree: PathBuf,
    bin: String,
}

impl Built {
    /// Throws the copy away: it was not put in place.
    pub fn discard(self) {
        skills::discard(Some(self.tree));
    }
}

/// Downloads every package of `entry` for this Mac, checks each against its pin and builds the
/// adapter in a folder of the staging area. Refused, with nothing left, when `node` is missing or
/// too old, a download is not what the list pins, a package has an install script or needs a
/// package the list does not pin. Slow: on a thread of its own.
pub fn build(entry: &AdapterEntry) -> Result<Built, Refusal> {
    runtime(entry.node).map_err(|why| ("no_runtime", why))?;
    let packages = wanted(entry);
    let names: HashSet<&str> = packages.iter().map(|p| folder_name(&p.path)).collect();
    let tree = skills::staging().map_err(failed)?;
    let made = (|| {
        sys::create_private_dir(&tree).map_err(failed)?;
        for pin in &packages {
            catalog::pinned((&pin.url, pin.size, &pin.sha256), |unpacked| {
                // Tarballs of npm hold one folder, `package`.
                let from = unpacked.join("package");
                check_package(pin, &from, &names)?;
                let to = tree.join(&pin.path);
                fs::create_dir_all(to.parent().ok_or_else(|| failed("no parent"))?)
                    .map_err(failed)?;
                fs::rename(&from, &to).map_err(failed)
            })?;
        }
        if !fs::symlink_metadata(tree.join(&entry.bin)).is_ok_and(|m| m.is_file()) {
            return Err(failed(format!("the package has no {}", entry.bin)));
        }
        let mark = json!({"version": entry.version, "bin": entry.bin});
        fs::write(tree.join(MARK), mark.to_string()).map_err(failed)
    })();
    match made {
        Ok(()) => Ok(Built {
            name: entry.name.clone(),
            version: entry.version.clone(),
            tree,
            bin: entry.bin.clone(),
        }),
        Err(e) => {
            skills::discard(Some(tree));
            Err(e)
        }
    }
}

/// `text` as one word of a shell script.
fn quoted(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

/// Puts the finished copy in place of the one installed (if any), and writes the script that
/// starts it. Quick: no download, only moves.
pub fn put(built: Built) -> io::Result<()> {
    let (dest, bin_dir) = (folder(&built.name), bin_dir());
    sys::create_private_dir(&bin_dir)?;
    let old = match fs::symlink_metadata(&dest) {
        Ok(_) => {
            let out = skills::staging()?;
            fs::rename(&dest, &out)?;
            Some(out)
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => return Err(e),
    };
    if let Err(e) = fs::rename(&built.tree, &dest) {
        if let Some(old) = &old {
            let _ = fs::rename(old, &dest);
        }
        skills::discard(Some(built.tree));
        return Err(e);
    }
    skills::discard(old);
    let script = format!(
        "#!/bin/sh\nexec node {} \"$@\"\n",
        quoted(&dest.join(&built.bin).to_string_lossy())
    );
    let temp = bin_dir.join(format!(".{}.new", built.name));
    let _ = fs::remove_file(&temp);
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o755)
        .open(&temp)?;
    file.write_all(script.as_bytes())?;
    fs::rename(&temp, bin_dir.join(&built.name))
}

/// Deletes Bate's copy of `name`; false when there was none. The person's own copy on their
/// `PATH` is not touched.
pub fn remove(name: &str) -> io::Result<bool> {
    match fs::remove_file(bin_dir().join(name)) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    let out = skills::staging()?;
    match fs::rename(folder(name), &out) {
        Ok(()) => {
            skills::discard(Some(out));
            Ok(true)
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_is_one_word_of_a_script() {
        assert_eq!(quoted("/a b/c"), "'/a b/c'");
        assert_eq!(quoted("/it's"), r"'/it'\''s'");
    }

    #[test]
    fn a_package_is_asked_for_by_its_folders_name() {
        assert_eq!(folder_name("node_modules/@a/b"), "@a/b");
        assert_eq!(folder_name("node_modules/x/node_modules/y"), "y");
    }
}
