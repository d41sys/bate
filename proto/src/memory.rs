//! Memory: a bot's notes, the Markdown files in `memory/` in its folder. The rules are in
//! `PROTOCOL.md`.

/// The folder in a bot's folder that holds its notes.
pub const DIR: &str = "memory";

/// The folder in a project's folder that holds what Bate keeps there; the project's notes are
/// in `DIR` inside it.
pub const PROJECT: &str = ".bate";

/// Whether a file name is a note's: ending in `.md`, at most 100 bytes, no `/` or control
/// character, not starting with a dot. The engine shows the model exactly the notes the
/// window's panel lists, so both ask here.
pub fn is_note_name(name: &str) -> bool {
    name.len() <= 100
        && name.len() > ".md".len()
        && name.ends_with(".md")
        && !name.starts_with('.')
        && !name.chars().any(|c| c == '/' || c.is_control())
}
