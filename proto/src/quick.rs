//! Quick Ask's global shortcut: how it is written down (`Ctrl+Alt+Space`), checked, and turned
//! into what macOS registers. The service stores the text; the window registers it.

use serde::Serialize;
use std::fmt;

/// What the person set for Quick Ask in Settings. Everything is off until they turn it on.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    /// A text `Shortcut::parse` takes; `None`: no shortcut.
    pub shortcut: Option<String>,
    /// The text selected in the app in front goes into the question as a quote.
    pub selection: bool,
    /// A picture of the screen or of a window goes with the question.
    pub screenshot: bool,
}

/// The keys a shortcut may use: a letter, a digit, Space or F1 to F12, each with the macOS key
/// code of its place on the keyboard (`kVK_ANSI_*`).
const KEYS: [(&str, u32); 49] = [
    ("A", 0x00),
    ("S", 0x01),
    ("D", 0x02),
    ("F", 0x03),
    ("H", 0x04),
    ("G", 0x05),
    ("Z", 0x06),
    ("X", 0x07),
    ("C", 0x08),
    ("V", 0x09),
    ("B", 0x0b),
    ("Q", 0x0c),
    ("W", 0x0d),
    ("E", 0x0e),
    ("R", 0x0f),
    ("Y", 0x10),
    ("T", 0x11),
    ("1", 0x12),
    ("2", 0x13),
    ("3", 0x14),
    ("4", 0x15),
    ("6", 0x16),
    ("5", 0x17),
    ("9", 0x19),
    ("7", 0x1a),
    ("8", 0x1c),
    ("0", 0x1d),
    ("O", 0x1f),
    ("U", 0x20),
    ("I", 0x22),
    ("P", 0x23),
    ("L", 0x25),
    ("J", 0x26),
    ("K", 0x28),
    ("N", 0x2d),
    ("M", 0x2e),
    ("Space", 0x31),
    ("F1", 0x7a),
    ("F2", 0x78),
    ("F3", 0x63),
    ("F4", 0x76),
    ("F5", 0x60),
    ("F6", 0x61),
    ("F7", 0x62),
    ("F8", 0x64),
    ("F9", 0x65),
    ("F10", 0x6d),
    ("F11", 0x67),
    ("F12", 0x6f),
];

/// Carbon's modifier bits (`controlKey`, `optionKey`, `shiftKey`, `cmdKey`).
const CTRL: u32 = 4096;
const ALT: u32 = 2048;
const SHIFT: u32 = 512;
const CMD: u32 = 256;
/// Written in this order, so one shortcut has one text.
const MODIFIERS: [(&str, u32); 4] = [("Ctrl", CTRL), ("Alt", ALT), ("Shift", SHIFT), ("Cmd", CMD)];

/// A key with at least one of Ctrl, Alt and Cmd (Shift alone would take a plain capital letter).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shortcut {
    modifiers: u32,
    key: usize,
}

impl Shortcut {
    /// `Ctrl+Alt+Space`: modifiers and the key, in any order; case does not matter.
    pub fn parse(text: &str) -> Result<Shortcut, String> {
        let (mut modifiers, mut key) = (0, None);
        for part in text.split('+') {
            let part = part.trim();
            if let Some((_, bit)) = MODIFIERS.iter().find(|(n, _)| n.eq_ignore_ascii_case(part)) {
                if modifiers & bit != 0 {
                    return Err(format!("`{part}` is there twice"));
                }
                modifiers |= bit;
            } else if let Some(at) = KEYS.iter().position(|(n, _)| n.eq_ignore_ascii_case(part)) {
                if key.replace(at).is_some() {
                    return Err("a shortcut has one key".into());
                }
            } else {
                return Err(format!(
                    "`{part}` is not a key a shortcut may use: a letter, a digit, Space or F1 to F12"
                ));
            }
        }
        let key = key.ok_or("a shortcut needs a key")?;
        if modifiers & (CTRL | ALT | CMD) == 0 {
            return Err("a shortcut needs Ctrl, Alt or Cmd".into());
        }
        Ok(Shortcut { modifiers, key })
    }

    /// What macOS takes to register it: the key's code and Carbon's modifier bits.
    pub fn carbon(self) -> (u32, u32) {
        (KEYS[self.key].1, self.modifiers)
    }
}

impl fmt::Display for Shortcut {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (name, bit) in MODIFIERS {
            if self.modifiers & bit != 0 {
                write!(f, "{name}+")?;
            }
        }
        f.write_str(KEYS[self.key].0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_shortcut_has_one_text_and_the_codes_macos_takes() {
        let s = Shortcut::parse("cmd + shift+space").unwrap();
        assert_eq!(s.to_string(), "Shift+Cmd+Space");
        assert_eq!(s.carbon(), (0x31, 512 | 256));
        assert_eq!(
            Shortcut::parse("Ctrl+Alt+F12").unwrap().carbon(),
            (0x6f, 4096 | 2048)
        );
        // The text it gives is the text it takes.
        assert_eq!(Shortcut::parse(&s.to_string()).unwrap(), s);
    }

    #[test]
    fn what_is_not_a_shortcut_is_refused_with_the_reason() {
        for (bad, why) in [
            ("", "not a key"),
            ("Cmd", "needs a key"),
            ("Space", "needs Ctrl, Alt or Cmd"),
            ("Shift+A", "needs Ctrl, Alt or Cmd"),
            ("Cmd+Cmd+A", "twice"),
            ("Cmd+A+B", "one key"),
            ("Cmd+Enter", "not a key"),
            ("Cmd+", "not a key"),
        ] {
            let error = Shortcut::parse(bad).unwrap_err();
            assert!(error.contains(why), "{bad}: {error}");
        }
    }

    #[test]
    fn every_key_has_its_own_code() {
        let mut codes: Vec<u32> = KEYS.iter().map(|k| k.1).collect();
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(codes.len(), KEYS.len());
    }
}
