//! Skills: what the model is told about the skill folders in `skills/` in the bot's folder.
//! Only names and descriptions go into every turn; the model reads a skill's file when a task
//! calls for it.

use crate::memory::open;
use proto::skills::{DIR, FILE, parse};
use std::fs;
use std::io::Read;
use std::path::Path;

/// Most skills listed, and most bytes read of one `SKILL.md` to find its description.
const MOST: usize = 50;
const MAX_FILE: u64 = 64 * 1024;

/// The part of the system prompt about skills; empty when the bot has none.
pub fn prompt(folder: &Path) -> String {
    let dir = folder.join(DIR);
    // As with memory: only plain folders and files count, never what a link points to.
    if !fs::symlink_metadata(&dir).is_ok_and(|meta| meta.is_dir()) {
        return String::new();
    }
    let mut found: Vec<_> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .filter_map(|entry| {
            let mut text = String::new();
            let file = open(&entry.path().join(FILE)).ok()?;
            file.take(MAX_FILE).read_to_string(&mut text).ok()?;
            let skill = parse(&text).ok()?;
            // The folder is how the model finds the file, so it has to carry the name.
            (entry.file_name().to_str() == Some(skill.name.as_str())).then_some(skill)
        })
        .collect();
    if found.is_empty() {
        return String::new();
    }
    found.sort_by(|a, b| a.name.cmp(&b.name));
    let more = found.len().saturating_sub(MOST);
    found.truncate(MOST);
    let mut text = String::from(
        "\n\nYou have skills: instructions for kinds of task. Before you start a task that one \
         of them is for, read its file with the read tool and follow it.",
    );
    for skill in found {
        text.push_str(&format!(
            "\n- {} ({DIR}/{}/{FILE}): {}",
            skill.name, skill.name, skill.description
        ));
    }
    if more > 0 {
        text.push_str(&format!(
            "\n({more} more are not listed; their folders are in {DIR}/.)"
        ));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installed_skills_are_listed_by_name_with_where_to_read_them() {
        let folder = std::env::temp_dir().join(format!("agentcore-skills-{}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        assert_eq!(prompt(&folder), "");
        let skill = |name: &str, text: &str| {
            fs::create_dir_all(folder.join(DIR).join(name)).unwrap();
            fs::write(folder.join(DIR).join(name).join(FILE), text).unwrap();
        };
        skill(
            "b-skill",
            "---\nname: b-skill\ndescription: For B.\n---\nDo B.",
        );
        skill(
            "a-skill",
            "---\nname: a-skill\ndescription: For A.\n---\nDo A, secretly.",
        );
        skill("broken", "no front matter");
        skill(
            "renamed",
            "---\nname: other\ndescription: Lies about its name.\n---\n",
        );
        // A linked folder is not a skill, whatever it points to.
        std::os::unix::fs::symlink(
            folder.join(DIR).join("a-skill"),
            folder.join(DIR).join("link"),
        )
        .unwrap();
        let text = prompt(&folder);
        assert!(
            text.ends_with("\n- a-skill (skills/a-skill/SKILL.md): For A.\n- b-skill (skills/b-skill/SKILL.md): For B."),
            "{text}"
        );
        // Only the description: the instructions are read when needed.
        assert!(!text.contains("secretly") && !text.contains("Lies"));
        fs::remove_dir_all(&folder).unwrap();
    }
}
