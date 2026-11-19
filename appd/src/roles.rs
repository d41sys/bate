//! Roles: files that describe ready-made bots a project can add. The format is in
//! `docs/roles.md`.

use crate::sys::Dir;
use proto::permissions::{Kind, Level};
use serde::Serialize;
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};

/// Most bytes of a role file.
const MAX_FILE: u64 = 64 * 1024;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Role {
    pub path: PathBuf,
    pub name: String,
    pub instructions: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    /// Names of skills, each a folder beside the role file.
    pub skills: Vec<String>,
    pub levels: HashMap<Kind, Level>,
    /// The kinds whose level allows more than a new bot's: applied only if the person agrees.
    pub raises: Vec<Kind>,
}

/// Reads the role file at the whole path `path`.
pub fn read(path: &Path) -> Result<Role, String> {
    let fail = |e: String| format!("{}: {e}", path.display());
    let (Some(parent), Some(name)) = (path.parent(), path.file_name().and_then(|n| n.to_str()))
    else {
        return Err(fail("not a file's path".into()));
    };
    if !path.is_absolute() {
        return Err(fail("give the whole path, starting with /".into()));
    }
    let dir = Dir::open(parent).map_err(|e| fail(format!("cannot open its folder: {e}")))?;
    let file = dir
        .regular(name)
        .map_err(|e| fail(format!("cannot open it: {e}")))?
        .ok_or_else(|| fail("there is no such plain file".into()))?;
    let mut text = String::new();
    file.take(MAX_FILE + 1)
        .read_to_string(&mut text)
        .map_err(|e| fail(format!("cannot read it as text: {e}")))?;
    if text.len() as u64 > MAX_FILE {
        return Err(fail("it is larger than 64 KiB".into()));
    }
    let mut role = parse(&text).map_err(fail)?;
    role.path = path.to_owned();
    Ok(role)
}

/// How much a level lets a bot do without the person.
fn reach(level: Level) -> u8 {
    match level {
        Level::Never => 0,
        Level::Ask => 1,
        Level::Always => 2,
    }
}

// shortcut: one-line `key: value` front matter, as in a skill's; use a YAML parser when a role needs more.
fn parse(text: &str) -> Result<Role, String> {
    let mut lines = text.split_inclusive('\n');
    if lines.next().map(str::trim_end) != Some("---") {
        return Err("a role file must start with a `---` line".into());
    }
    let mut head = Vec::new();
    let mut closed = false;
    for line in lines.by_ref() {
        if line.trim_end() == "---" {
            closed = true;
            break;
        }
        head.push(line.trim());
    }
    if !closed {
        return Err("the front matter has no closing `---` line".into());
    }
    let instructions = lines.collect::<String>().trim().to_owned();
    if instructions.chars().count() > 8000 {
        return Err("the instructions may be at most 8000 characters".into());
    }
    let mut role = Role {
        path: PathBuf::new(),
        name: String::new(),
        instructions,
        model: None,
        effort: None,
        skills: Vec::new(),
        levels: HashMap::new(),
        raises: Vec::new(),
    };
    let mut seen = Vec::new();
    for line in head
        .into_iter()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
    {
        let Some((key, value)) = line.split_once(':') else {
            return Err(format!("`{line}` is not `key: value`"));
        };
        let key = key.trim();
        let value = value.trim().trim_matches(|c| c == '"' || c == '\'').trim();
        if seen.contains(&key) {
            return Err(format!("`{key}` is given twice"));
        }
        seen.push(key);
        match key {
            "name" => role.name = value.to_owned(),
            "model" => role.model = Some(value.to_owned()),
            "effort" => role.effort = Some(value.to_owned()),
            "skills" => {
                for skill in value.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                    proto::skills::check_name(skill)?;
                    if !role.skills.iter().any(|s| s == skill) {
                        role.skills.push(skill.to_owned());
                    }
                }
            }
            _ => {
                let kind: Kind = serde_json::from_value(key.into())
                    .map_err(|_| format!("`{key}` is not a key of a role"))?;
                let level: Level = serde_json::from_value(value.into()).map_err(|_| {
                    format!("`{key}` must be `always`, `ask` or `never`, not `{value}`")
                })?;
                if kind.always_asks() && level == Level::Always {
                    return Err(format!(
                        "`{key}` is asked about every time: use `ask` or `never`"
                    ));
                }
                if reach(level) > reach(kind.default_level()) {
                    role.raises.push(kind);
                }
                role.levels.insert(kind, level);
            }
        }
    }
    if role.name.is_empty() || role.name.chars().count() > 80 {
        return Err("`name` must be 1 to 80 characters".into());
    }
    Ok(role)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_role_gives_name_instructions_levels_model_and_skills_or_the_reason_it_cannot() {
        let role = parse(
            "---\nname: Reviewer\n# a comment\nmodel: gpt-5\neffort: high\nskills: code-review, pdf, pdf\nread: always\nwrite: never\nrun: always\n---\nReview every change.\n",
        )
        .unwrap();
        assert_eq!(role.name, "Reviewer");
        assert_eq!(role.instructions, "Review every change.");
        assert_eq!(
            (role.model.as_deref(), role.effort.as_deref()),
            (Some("gpt-5"), Some("high"))
        );
        assert_eq!(role.skills, ["code-review", "pdf"]);
        assert_eq!(role.levels[&Kind::Write], Level::Never);
        // `read: always` is a new bot's own level; only `run` allows more.
        assert_eq!(role.raises, [Kind::Run]);

        for (text, reason) in [
            ("name: A\n---\n", "must start with"),
            ("---\nname: A\n", "no closing"),
            ("---\n---\nHi", "`name` must be"),
            ("---\nname: A\nwirte: always\n---\n", "`wirte` is not a key"),
            ("---\nname: A\nrun: sometimes\n---\n", "`run` must be"),
            ("---\nname: A\nname: B\n---\n", "given twice"),
            ("---\nname: A\napple_change: always\n---\n", "every time"),
            ("---\nname: A\nskills: ../x\n---\n", "a skill's `name`"),
            ("---\nname A\n---\n", "is not `key: value`"),
        ] {
            let e = parse(text).unwrap_err();
            assert!(e.contains(reason), "{text:?}: {e}");
        }
        let long = format!("---\nname: A\n---\n{}", "x".repeat(8001));
        assert!(parse(&long).unwrap_err().contains("8000"));
    }
}
