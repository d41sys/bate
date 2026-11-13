//! Signing in to MCP servers that use OAuth: finding the server's sign-in address, the
//! exchange of the browser's code for tokens, and their renewal. All of it talks to the
//! network, so none of it runs on the service's one thread. The tokens stay in the service:
//! the bot's engine talks to a local proxy (`hooks`), which adds them.

use crate::sys::{now_ms, random_hex};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(15);
/// Most bytes read of any answer from a sign-in server.
const MAX_ANSWER: u64 = 256 * 1024;

/// What the service keeps for a signed-in connector. Never sent to a window or an engine.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tokens {
    pub access: String,
    pub refresh: Option<String>,
    /// Unix ms after which `access` is no longer sent without renewing it first.
    pub expires_ms: Option<u64>,
    pub token_endpoint: String,
    pub client_id: String,
    /// The MCP server's URL, which the tokens are for.
    pub resource: String,
}

/// A sign-in under way, from the address the browser is sent to until it comes back.
#[derive(Clone, Debug)]
pub struct Pending {
    pub bot: String,
    pub name: String,
    pub started_ms: u64,
    verifier: String,
    token_endpoint: String,
    client_id: String,
    redirect: String,
    resource: String,
}

/// The scheme and the host (no port) of an address. None for one with a user name in it, or
/// with anything else that could make it read as another host than it goes to.
fn host(url: &str) -> Option<(&str, &str)> {
    let (scheme, rest) = url.split_once("://")?;
    let authority = rest.split(['/', '?', '#']).next()?;
    let odd = |c: char| c == '@' || c == '\\' || c.is_whitespace() || c.is_control();
    if authority.is_empty() || authority.contains(odd) {
        return None;
    }
    let host = match authority.strip_prefix('[') {
        Some(_) => &authority[..=authority.find(']')?],
        None => authority.split(':').next()?,
    };
    matches!(scheme, "http" | "https").then_some((scheme, host))
}

/// An address on this Mac.
pub fn local(url: &str) -> bool {
    host(url).is_some_and(|(_, host)| matches!(host, "127.0.0.1" | "localhost" | "[::1]"))
}

/// An address is fit to send secrets to: `https`, or this Mac.
pub fn safe(url: &str) -> bool {
    local(url) || host(url).is_some_and(|(scheme, _)| scheme == "https")
}

/// As `safe`, for an address that the server at `url` named: a server elsewhere may not
/// point the service at this Mac.
fn fit(named: &str, url: &str) -> bool {
    safe(named) && (local(url) || !local(named))
}

/// `scheme://host[:port]` of a URL, and the path after it (no query).
fn split(url: &str) -> Option<(&str, &str)> {
    let start = url.find("://")? + 3;
    let end = url[start..].find('/').map_or(url.len(), |i| start + i);
    let path = url[end..].split(['?', '#']).next().unwrap_or("");
    Some((&url[..end], path.trim_end_matches('/')))
}

fn agent() -> ureq::Agent {
    let config = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(TIMEOUT))
        // A redirect could carry a code or a token to another host.
        .max_redirects(0)
        .build();
    ureq::Agent::new_with_config(config)
}

fn json_of(response: ureq::http::Response<ureq::Body>, what: &str) -> Result<Value, String> {
    use std::io::Read;
    let status = response.status();
    let mut text = String::new();
    response
        .into_body()
        .into_reader()
        .take(MAX_ANSWER)
        .read_to_string(&mut text)
        .map_err(|e| format!("{what}: {e}"))?;
    if !status.is_success() {
        let shown: String = text.chars().take(300).collect();
        return Err(format!("{what} answered {status}: {}", shown.trim()));
    }
    serde_json::from_str(&text).map_err(|e| format!("{what} sent a bad answer: {e}"))
}

fn get(url: &str) -> Result<Value, String> {
    let response = agent()
        .get(url)
        .header("Accept", "application/json")
        .call()
        .map_err(|e| format!("cannot reach {url}: {e}"))?;
    json_of(response, url)
}

/// A field that must be an address fit for secrets.
fn endpoint(meta: &Value, key: &str, url: &str) -> Result<String, String> {
    match meta[key].as_str() {
        Some(named) if fit(named, url) => Ok(named.to_owned()),
        Some(url) => Err(format!("`{key}` is not https: {url}")),
        None => Err(format!("the sign-in server names no `{key}`")),
    }
}

/// Finds where `url`'s server signs users in, registers this app there unless `client_id` is
/// given, and makes the address to open in the browser. `redirect` is where the browser is
/// to come back to.
pub fn begin(
    bot: &str,
    name: &str,
    url: &str,
    client_id: Option<&str>,
    redirect: &str,
) -> Result<(String, String, Pending), String> {
    if !safe(url) {
        return Err("signing in needs an https address".into());
    }
    let (origin, path) = split(url).ok_or("not a URL")?;
    // The server says who signs its users in (RFC 9728); an older server does it itself.
    let resource_meta = get(&format!(
        "{origin}/.well-known/oauth-protected-resource{path}"
    ))
    .or_else(|_| get(&format!("{origin}/.well-known/oauth-protected-resource")));
    let issuer = resource_meta
        .ok()
        .and_then(|meta| meta["authorization_servers"][0].as_str().map(str::to_owned))
        .unwrap_or_else(|| origin.to_owned());
    if !fit(&issuer, url) {
        return Err(format!("the sign-in server is not https: {issuer}"));
    }
    let (as_origin, as_path) = split(&issuer).ok_or("the sign-in server is not a URL")?;
    let meta = get(&format!(
        "{as_origin}/.well-known/oauth-authorization-server{as_path}"
    ))
    .or_else(|_| get(&format!("{issuer}/.well-known/openid-configuration")))
    .map_err(|e| format!("this server does not say how to sign in: {e}"))?;
    let authorize = endpoint(&meta, "authorization_endpoint", url)?;
    let token_endpoint = endpoint(&meta, "token_endpoint", url)?;
    let client_id = match client_id {
        Some(id) => id.to_owned(),
        None => {
            let register = endpoint(&meta, "registration_endpoint", url).map_err(|_| {
                "this server does not register apps by itself: give the connector a `clientId`"
                    .to_owned()
            })?;
            let body = serde_json::json!({
                "client_name": "Bate",
                "redirect_uris": [redirect],
                "grant_types": ["authorization_code", "refresh_token"],
                "response_types": ["code"],
                "token_endpoint_auth_method": "none",
            });
            let response = agent()
                .post(&register)
                .header("Content-Type", "application/json")
                .send(body.to_string().as_bytes())
                .map_err(|e| format!("cannot reach {register}: {e}"))?;
            json_of(response, &register)?["client_id"]
                .as_str()
                .ok_or("the sign-in server gave no `client_id`")?
                .to_owned()
        }
    };
    let (state, verifier) = (random_hex(16), random_hex(32));
    let challenge = base64url(&sha256(verifier.as_bytes()));
    let mut address = authorize;
    address.push(if address.contains('?') { '&' } else { '?' });
    address.push_str(&form(&[
        ("response_type", "code"),
        ("client_id", &client_id),
        ("redirect_uri", redirect),
        ("state", &state),
        ("code_challenge", &challenge),
        ("code_challenge_method", "S256"),
        ("resource", url),
    ]));
    let pending = Pending {
        bot: bot.to_owned(),
        name: name.to_owned(),
        started_ms: now_ms(),
        verifier,
        token_endpoint,
        client_id,
        redirect: redirect.to_owned(),
        resource: url.to_owned(),
    };
    Ok((address, state, pending))
}

/// Why there are no new tokens. `refused`: the sign-in server said no, so the same request
/// will not do better later.
pub struct Failed {
    pub refused: bool,
    pub why: String,
}

/// Trades the code the browser came back with for tokens.
pub fn finish(pending: &Pending, code: &str) -> Result<Tokens, String> {
    let granted = grant(
        &pending.token_endpoint,
        &pending.client_id,
        &pending.resource,
        None,
        &[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", &pending.redirect),
            ("code_verifier", &pending.verifier),
        ],
    );
    granted.map_err(|failed| failed.why)
}

/// New tokens for old, when the server gave a way to renew.
pub fn renew(old: &Tokens) -> Result<Tokens, Failed> {
    let refresh = old.refresh.as_deref().ok_or(Failed {
        refused: true,
        why: "sign in again".into(),
    })?;
    grant(
        &old.token_endpoint,
        &old.client_id,
        &old.resource,
        Some(refresh),
        &[("grant_type", "refresh_token"), ("refresh_token", refresh)],
    )
}

fn grant(
    token_endpoint: &str,
    client_id: &str,
    resource: &str,
    old_refresh: Option<&str>,
    fields: &[(&str, &str)],
) -> Result<Tokens, Failed> {
    let mut all = fields.to_vec();
    all.extend([("client_id", client_id), ("resource", resource)]);
    let response = agent()
        .post(token_endpoint)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .header("Accept", "application/json")
        .send(form(&all).as_bytes())
        .map_err(|e| Failed {
            refused: false,
            why: format!("cannot reach {token_endpoint}: {e}"),
        })?;
    let refused = response.status().is_client_error();
    let answer = json_of(response, "the sign-in server").map_err(|why| Failed { refused, why })?;
    let access = answer["access_token"].as_str().ok_or(Failed {
        refused: true,
        why: "the sign-in server gave no access token".into(),
    })?;
    Ok(Tokens {
        access: access.to_owned(),
        // A server may keep the old one valid and send none.
        refresh: answer["refresh_token"]
            .as_str()
            .or(old_refresh)
            .map(str::to_owned),
        expires_ms: answer["expires_in"]
            .as_u64()
            .map(|seconds| now_ms() + seconds.saturating_mul(1000)),
        token_endpoint: token_endpoint.to_owned(),
        client_id: client_id.to_owned(),
        resource: resource.to_owned(),
    })
}

/// `a=b&c=d`, percent-encoded.
pub fn form(fields: &[(&str, &str)]) -> String {
    let encode = |text: &str| {
        text.bytes()
            .map(|b| match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                    (b as char).to_string()
                }
                _ => format!("%{b:02X}"),
            })
            .collect::<String>()
    };
    let pairs: Vec<String> = fields
        .iter()
        .map(|(key, value)| format!("{}={}", encode(key), encode(value)))
        .collect();
    pairs.join("&")
}

/// Undoes `form`'s encoding of one value (`+` is a space).
pub fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = bytes
            .get(i + 1..i + 3)
            .and_then(|h| std::str::from_utf8(h).ok());
        match (bytes[i], hex.and_then(|h| u8::from_str_radix(h, 16).ok())) {
            (b'%', Some(byte)) => {
                out.push(byte);
                i += 2;
            }
            (b'+', _) => out.push(b' '),
            (byte, _) => out.push(byte),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn sha256(data: &[u8]) -> [u8; 32] {
    // macOS's own (CommonCrypto, part of libSystem): no crate for one hash.
    unsafe extern "C" {
        fn CC_SHA256(data: *const u8, len: u32, out: *mut u8) -> *mut u8;
    }
    let mut out = [0; 32];
    // SAFETY: `data` is valid for `len` bytes (a verifier is 64) and `out` for the 32 written.
    unsafe { CC_SHA256(data.as_ptr(), data.len() as u32, out.as_mut_ptr()) };
    out
}

fn base64url(data: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    for chunk in data.chunks(3) {
        let n = chunk
            .iter()
            .fold(0u32, |n, &byte| (n << 8) | u32::from(byte))
            << (8 * (3 - chunk.len()));
        for i in 0..=chunk.len() {
            out.push(ALPHABET[(n >> (18 - 6 * i)) as usize & 63] as char);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_challenge_is_the_verifiers_hash_as_the_standard_writes_it() {
        // RFC 7636, appendix B.
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        assert_eq!(
            base64url(&sha256(verifier.as_bytes())),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
        assert_eq!(base64url(b"a"), "YQ");
        assert_eq!(base64url(b"ab"), "YWI");
        assert_eq!(base64url(b"abc"), "YWJj");
    }

    #[test]
    fn secrets_go_to_https_or_this_mac_only() {
        for ok in [
            "https://mcp.example.com/x",
            "http://127.0.0.1:9/x",
            "http://localhost/x",
            "http://[::1]/x",
            "http://[::1]:8080/x",
        ] {
            assert!(safe(ok), "{ok}");
        }
        for bad in [
            "http://mcp.example.com/x",
            "http://127.0.0.1.evil.com/x",
            "ftp://x",
            "http://localhost.evil.com",
            "http://localhost:1@192.168.1.1/x",
            "https://localhost@evil.com/x",
            "http://127.0.0.1\\@evil.com/x",
            "http://[::1/x",
            "HTTP://evil.com/x",
            "javascript://localhost/x",
        ] {
            assert!(!safe(bad), "{bad}");
        }
        // What a server elsewhere names may not be on this Mac.
        assert!(fit(
            "https://auth.example.com/x",
            "https://mcp.example.com/"
        ));
        assert!(fit("http://127.0.0.1:1/x", "http://localhost:2/mcp"));
        assert!(!fit("http://127.0.0.1:1/x", "https://mcp.example.com/"));
        assert!(!fit("https://localhost/x", "https://mcp.example.com/"));
        assert_eq!(
            split("https://a.com:8/mcp/v1/?x=1"),
            Some(("https://a.com:8", "/mcp/v1"))
        );
        assert_eq!(split("https://a.com"), Some(("https://a.com", "")));
        assert_eq!(form(&[("a b", "c&d=é")]), "a%20b=c%26d%3D%C3%A9");
        assert_eq!(decode("c%26d%3D%C3%A9+x%2"), "c&d=é x%2");
    }
}
