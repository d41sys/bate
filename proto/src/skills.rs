//! Skills: folders with a `SKILL.md` that tell a bot how to do a kind of task. Installed per
//! bot under `skills/<name>/` in its folder. The rules are in `PROTOCOL.md`.

use serde::{Deserialize, Serialize};

/// The folder in a bot's folder that holds its skills.
pub const DIR: &str = "skills";
/// The file every skill has: a front matter with `name` and `description`, then instructions.
pub const FILE: &str = "SKILL.md";
/// Most characters of a description (the model sees every skill's description every turn).
pub const MAX_DESCRIPTION: usize = 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Skill {
    /// 1 to 64 of a-z, 0-9 and `-`: also the skill's folder name.
    pub name: String,
    /// When to use it, on one line.
    pub description: String,
}

/// Reads the front matter of a `SKILL.md`: the lines between a first line `---` and the next
/// `---`, as `key: value`. Only `name` and `description` are used.
// shortcut: one-line values only (no YAML block or quoted multi-line strings); use a YAML
// parser when a real skill needs them.
pub fn parse(text: &str) -> Result<Skill, String> {
    let mut lines = text.lines();
    if lines.next().map(str::trim_end) != Some("---") {
        return Err(format!("{FILE} must start with a `---` line"));
    }
    let (mut name, mut description, mut closed) = (None, None, false);
    for line in lines {
        if line.trim_end() == "---" {
            closed = true;
            break;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim().trim_matches(|c| c == '"' || c == '\'').trim();
        match key.trim() {
            "name" => name = Some(value.to_owned()),
            "description" => description = Some(value.to_owned()),
            _ => {}
        }
    }
    if !closed {
        return Err(format!(
            "the front matter of {FILE} has no closing `---` line"
        ));
    }
    let name = name.ok_or(format!("{FILE} has no `name`"))?;
    let description = description.ok_or(format!("{FILE} has no `description`"))?;
    check_name(&name)?;
    let length = description.chars().count();
    if length == 0 || length > MAX_DESCRIPTION {
        return Err(format!(
            "`description` must be 1 to {MAX_DESCRIPTION} characters"
        ));
    }
    Ok(Skill { name, description })
}

pub fn check_name(name: &str) -> Result<(), String> {
    let ok = (1..=64).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    if ok {
        Ok(())
    } else {
        Err("a skill's `name` must be 1 to 64 characters of a-z, 0-9 and -".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_front_matter_gives_name_and_description() {
        let text = "---\nname: pdf-forms\ndescription: \"Fill in PDF forms: use for any .pdf with fields.\"\nlicense: MIT\n---\n# How\nname: not-this\n";
        assert_eq!(
            parse(text),
            Ok(Skill {
                name: "pdf-forms".into(),
                description: "Fill in PDF forms: use for any .pdf with fields.".into()
            })
        );
        assert_eq!(
            parse("---\r\nname: a\r\ndescription: b\r\n---\r\n")
                .unwrap()
                .name,
            "a"
        );
    }

    #[test]
    fn a_skill_file_without_what_is_needed_is_refused() {
        for (text, why) in [
            ("# Just text", "start with"),
            ("---\nname: a\ndescription: b\n", "closing"),
            ("---\ndescription: b\n---\n", "no `name`"),
            ("---\nname: a\n---\n", "no `description`"),
            ("---\nname: ../up\ndescription: b\n---\n", "a-z"),
            ("---\nname: Has Space\ndescription: b\n---\n", "a-z"),
            ("---\nname: a\ndescription:\n---\n", "1 to"),
        ] {
            let error = parse(text).unwrap_err();
            assert!(error.contains(why), "{text:?}: {error}");
        }
        let long = format!(
            "---\nname: a\ndescription: {}\n---\n",
            "x".repeat(MAX_DESCRIPTION + 1)
        );
        assert!(parse(&long).is_err());
    }
}
