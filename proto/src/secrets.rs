//! Secrets a person gives a bot for one site: the value goes to the Keychain, the bot is told
//! only the name. The rules are in `PROTOCOL.md` (Secrets for a site).

use serde::{Deserialize, Serialize};

/// Most characters of a name.
pub const MAX_NAME: usize = 64;
/// Most characters of a value: what one `type` action may put in a field.
pub const MAX_VALUE: usize = 10_000;
/// What replaces a value that comes back in text for the bot.
pub fn marker(name: &str) -> String {
    format!("[secret {name}]")
}

/// What a `question` asks for when it asks for a secret.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Wanted {
    pub name: String,
    /// What the engine wrote; the service puts the host in its place before anyone sees it.
    pub site: String,
}

/// A name the bot can refer to: letters, digits, `-`, `_` and `.`.
pub fn name_ok(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_NAME
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

/// The host of a site given as a host (`example.com`) or as an http or https address, in
/// lower case, without port or path. `None` for anything else: a user name, an IP address in
/// brackets, an empty part, a character a host does not have.
// shortcut: ASCII hosts only (a name with non-ASCII letters is written in punycode, `xn--…`);
// upgrade when a person needs to paste one as it is shown.
pub fn host(site: &str) -> Option<String> {
    let site = site.trim().to_ascii_lowercase();
    let rest = match site.split_once("://") {
        Some(("http" | "https", rest)) => rest,
        Some(_) => return None,
        None => &site,
    };
    let authority = rest.split(['/', '?', '#']).next()?;
    let host = match authority.split_once(':') {
        Some((host, port)) if !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) => host,
        Some(_) => return None,
        None => authority,
    };
    let label_ok = |l: &str| {
        !l.is_empty()
            && !l.starts_with('-')
            && !l.ends_with('-')
            && l.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
    };
    (host.len() <= 253 && host.split('.').all(label_ok)).then(|| host.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_site_is_reduced_to_its_host_and_nothing_odd_passes() {
        for (site, want) in [
            ("Example.COM", "example.com"),
            ("https://Login.Example.com:8443/a?b#c", "login.example.com"),
            (" http://127.0.0.1:3000 ", "127.0.0.1"),
        ] {
            assert_eq!(host(site).as_deref(), Some(want), "{site}");
        }
        for bad in [
            "",
            "file:///etc/hosts",
            "ftp://example.com",
            "https://user@example.com",
            "example.com.",
            "exa mple.com",
            "-example.com",
            "https://[::1]/",
            "example.com:80a",
            "https:///x",
        ] {
            assert_eq!(host(bad), None, "{bad}");
        }
    }

    #[test]
    fn a_name_is_plain() {
        assert!(name_ok("work-email.2"));
        for bad in ["", "a b", "a:b", "é", &"x".repeat(MAX_NAME + 1)] {
            assert!(!name_ok(bad), "{bad}");
        }
    }
}
