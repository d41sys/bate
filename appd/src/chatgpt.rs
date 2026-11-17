//! Sign in with ChatGPT, the way OpenAI documents for open-source local apps
//! (<https://developers.openai.com/siwc/token-sharing-open-source>), under Bate's own name:
//! registration and sign-in in the browser, renewal, sign-out, and the list of models. The
//! service uses it for bots (#8); the gate (#1) for its one request.

use crate::sys::{now_ms, random_hex};
use crate::{attach, oauth};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::{BufRead, BufReader, ErrorKind, Read, Write};
use std::net::TcpListener;
use std::time::{Duration, Instant};

const SCOPE: &str = "openid profile email offline_access resource.invoke chatgpt.tokens.use.direct";
/// The scope that lets a request count against the user's ChatGPT plan.
const PLAN: &str = "chatgpt.tokens.use.direct";
/// Most bytes read of any answer (the list of models alone is some hundred KiB).
const MAX_ANSWER: u64 = 16 << 20;
/// Most characters shown of an answer that is not a reply.
const MAX_SHOWN: usize = 2000;
/// How long the browser may take to come back.
const WAIT: Duration = Duration::from_secs(600);
/// What the sign-in server says of a refresh token that will never work again.
const ENDED: [&str; 6] = [
    "invalid_grant",
    "invalid_refresh_token",
    "token_expired",
    "refresh_token_expired",
    "refresh_token_invalidated",
    "refresh_token_reused",
];

fn env(name: &str, or: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| or.to_owned())
}

/// OpenAI's sign-in server (`BATE_CHATGPT_AUTH` names a stand-in).
pub fn auth() -> String {
    env("BATE_CHATGPT_AUTH", "https://auth.openai.com")
}

/// OpenAI's API (`BATE_CHATGPT_API` names a stand-in).
pub fn api() -> String {
    env("BATE_CHATGPT_API", "https://api.openai.com/v1")
}

/// What a sign-in leaves: a second sign-in needs no second registration, and a model call
/// no second sign-in.
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Saved {
    /// This Mac, as OpenAI is told: the same for every sign-in from it.
    pub host_id: String,
    /// The registration OpenAI issued to this user for Bate.
    pub client_id: String,
    /// Empty when signed out, or until a code has been traded for it.
    pub access_token: String,
    /// Renews the access token; replaced with every renewal.
    #[serde(default)]
    pub refresh_token: String,
    pub expires_ms: u64,
}

impl Saved {
    pub fn signed_in(&self) -> bool {
        !self.access_token.is_empty()
    }
}

/// A sign-in the browser has not come back from.
pub struct Pending {
    listener: TcpListener,
    auth: String,
    api: String,
    redirect: String,
    state: String,
    nonce: String,
    verifier: String,
    host_id: String,
    /// The registration this sign-in is for; none yet the first time.
    client_id: Option<String>,
    /// Set when another sign-in has begun: this one stops waiting and gives its port back.
    pub over: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

/// Why a renewal gave no tokens.
pub struct Failed {
    /// The sign-in is over: the tokens are to be forgotten.
    pub forget: bool,
    pub why: String,
}

pub fn agent() -> ureq::Agent {
    let config = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_connect(Some(Duration::from_secs(15)))
        // For the head of an answer; a reply may then stream for as long as it takes.
        .timeout_recv_response(Some(Duration::from_secs(120)))
        // A redirect could carry the code or the token to another host.
        .max_redirects(0)
        .build();
    ureq::Agent::new_with_config(config)
}

/// The status of an answer, with OpenAI's id for the request (its support asks for it), and
/// the answer's text.
pub fn text_of(response: ureq::http::Response<ureq::Body>) -> (String, String) {
    let id = response.headers().get("x-request-id");
    let id = id.and_then(|id| id.to_str().ok()).unwrap_or("none");
    let status = format!("{}, request id {id}", response.status().as_u16());
    let mut bytes = Vec::new();
    let mut body = response.into_body().into_reader().take(MAX_ANSWER);
    let _ = body.read_to_end(&mut bytes);
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

/// The start of `text`: enough to say what was refused.
pub fn shown(text: &str) -> String {
    text.chars().take(MAX_SHOWN).collect()
}

/// Starts a sign-in: listens for the browser, and gives the address to open in it. With
/// `old`, the sign-in is for that registration; without, Bate is registered first.
pub fn begin(auth: &str, api: &str, old: Option<&Saved>) -> Result<(Pending, String), String> {
    // The documented port; another when it is taken (only the port may differ).
    let listener = TcpListener::bind("127.0.0.1:1455")
        .or_else(|_| TcpListener::bind("127.0.0.1:0"))
        .map_err(|e| format!("cannot listen for the browser: {e}"))?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let pending = Pending {
        listener,
        auth: auth.to_owned(),
        api: api.to_owned(),
        redirect: format!("http://127.0.0.1:{port}/auth/callback"),
        state: random_hex(16),
        nonce: random_hex(16),
        verifier: random_hex(32),
        host_id: old.map_or_else(new_host_id, |s| s.host_id.clone()),
        client_id: old.map(|s| s.client_id.clone()),
        over: Default::default(),
    };
    let challenge = oauth::base64url(&oauth::sha256(pending.verifier.as_bytes()));
    let mut fields = vec![
        ("response_type", "code"),
        ("redirect_uri", pending.redirect.as_str()),
        ("scope", SCOPE),
        ("resource", api),
        ("state", pending.state.as_str()),
        ("nonce", pending.nonce.as_str()),
        ("code_challenge_method", "S256"),
        ("code_challenge", challenge.as_str()),
        ("ext_agent_host_id", pending.host_id.as_str()),
    ];
    match &pending.client_id {
        // No `id_token_hint`: the address is shown, and that token names the user.
        Some(id) => fields.push(("client_id", id.as_str())),
        None => fields.extend([
            ("client_id", "dynamic_agent_client"),
            ("agent_name_hint", "Bate"),
        ]),
    }
    let address = format!("{auth}/api/accounts/authorize?{}", oauth::form(&fields));
    Ok((pending, address))
}

/// The rest of a sign-in: waits for the browser to come back with a code and trades it for
/// tokens. A new registration is given to `keep` as soon as it is known: should the rest
/// fail, the next try does not register Bate again.
pub fn finish(
    pending: Pending,
    keep: impl FnOnce(&Saved) -> Result<(), String>,
) -> Result<Saved, String> {
    let query = come_back(&pending.listener, &pending.state, &pending.over)?;
    let field = |name: &str| param(&query, name);
    if let Some(error) = field("error") {
        let more = field("error_description").unwrap_or_default();
        return Err(format!("ChatGPT refused the sign-in: {error} {more}"));
    }
    let code = field("code").ok_or("the browser came back without a code")?;
    let (client_id, new) = match (&pending.client_id, field("client_id")) {
        (Some(old), Some(id)) if id != *old => {
            return Err("the browser came back with another client id".into());
        }
        (Some(old), _) => (old.clone(), false),
        (None, Some(id)) => (id, true),
        (None, None) => return Err("registration did not finish: no client id came back".into()),
    };
    let saved = Saved {
        host_id: pending.host_id.clone(),
        client_id,
        ..Saved::default()
    };
    if new {
        keep(&saved)?;
    }
    let fields = [
        ("grant_type", "authorization_code"),
        ("client_id", saved.client_id.as_str()),
        ("code", code.as_str()),
        ("code_verifier", pending.verifier.as_str()),
        ("redirect_uri", pending.redirect.as_str()),
        ("resource", pending.api.as_str()),
    ];
    let (ok, status, text) = grant(&pending.auth, &fields)?;
    if !ok {
        return Err(format!(
            "the code was refused ({status}):\n{}",
            shown(&text)
        ));
    }
    let tokens: Value = serde_json::from_str(&text).map_err(|e| format!("bad tokens: {e}"))?;
    let granted = tokens["scope"].as_str().unwrap_or("");
    if !granted.split(' ').any(|scope| scope == PLAN) {
        return Err(format!(
            "signed in, but use of the ChatGPT plan was not granted (scopes: {granted})"
        ));
    }
    // shortcut: the ID token's signature is not checked: it came straight from OpenAI over
    // TLS and only its nonce is used. Check it against the JWKS when the identity is kept.
    let claims = tokens["id_token"]
        .as_str()
        .and_then(|token| token.split('.').nth(1))
        .and_then(|part| attach::base64(&part.replace('-', "+").replace('_', "/")))
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok());
    if claims.as_ref().and_then(|c| c["nonce"].as_str()) != Some(pending.nonce.as_str()) {
        return Err("the ID token is not for this sign-in (another nonce)".into());
    }
    with_tokens(saved, &tokens)
}

/// Posts a grant to the token endpoint: whether it was given, the status, and the answer.
fn grant(auth: &str, fields: &[(&str, &str)]) -> Result<(bool, String, String), String> {
    let endpoint = format!("{auth}/api/accounts/oauth/token");
    let response = agent()
        .post(&endpoint)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .header("Accept", "application/json")
        .send(oauth::form(fields).as_bytes())
        .map_err(|e| format!("cannot reach {endpoint}: {e}"))?;
    let ok = response.status() == 200;
    let (status, text) = text_of(response);
    Ok((ok, status, text))
}

/// `saved` with the tokens of a token endpoint's answer. A refresh token the answer does
/// not replace stays.
fn with_tokens(mut saved: Saved, tokens: &Value) -> Result<Saved, String> {
    let token = tokens["access_token"].as_str();
    saved.access_token = token
        .filter(|t| !t.is_empty())
        .ok_or("the sign-in server gave no `access_token`")?
        .to_owned();
    if let Some(refresh) = tokens["refresh_token"].as_str() {
        saved.refresh_token = refresh.to_owned();
    }
    let life_ms = tokens["expires_in"]
        .as_u64()
        .unwrap_or(0)
        .saturating_mul(1000);
    saved.expires_ms = now_ms().saturating_add(life_ms);
    Ok(saved)
}

/// Trades the refresh token for new tokens. The old refresh token is used up by it: the
/// caller keeps the answer, and makes one renewal at a time.
pub fn renew(auth: &str, api: &str, saved: &Saved) -> Result<Saved, Failed> {
    let later = |why: String| Failed { forget: false, why };
    if saved.refresh_token.is_empty() {
        return Err(Failed {
            forget: true,
            why: "the ChatGPT sign-in cannot be renewed".into(),
        });
    }
    let fields = [
        ("grant_type", "refresh_token"),
        ("client_id", saved.client_id.as_str()),
        ("refresh_token", saved.refresh_token.as_str()),
        ("resource", api),
    ];
    let (ok, status, text) = grant(auth, &fields).map_err(later)?;
    let answer: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    if ok {
        return with_tokens(saved.clone(), &answer).map_err(later);
    }
    // OAuth's `error` is a word; OpenAI's own errors are an object with a `code`.
    let code = [&answer["error"], &answer["error"]["code"], &answer["code"]]
        .into_iter()
        .find_map(Value::as_str)
        .unwrap_or("");
    Err(Failed {
        forget: ENDED.contains(&code),
        why: format!("ChatGPT did not renew the sign-in ({status}): {code}"),
    })
}

/// Ends the renewable session at OpenAI. The registration stays.
pub fn revoke(auth: &str, saved: &Saved) -> Result<(), String> {
    let found = format!("{auth}/.well-known/openid-configuration");
    let response = agent()
        .get(&found)
        .call()
        .map_err(|e| format!("cannot reach {found}: {e}"))?;
    let (status, text) = text_of(response);
    let meta: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    let endpoint = meta["revocation_endpoint"]
        .as_str()
        .ok_or(format!("{found} names no revocation endpoint ({status})"))?;
    // The refresh token goes nowhere but to the server that issued it.
    if !endpoint.starts_with(&format!("{auth}/")) {
        return Err(format!(
            "the revocation endpoint is on another server: {endpoint}"
        ));
    }
    let body = oauth::form(&[
        ("token", saved.refresh_token.as_str()),
        ("token_type_hint", "refresh_token"),
        ("client_id", saved.client_id.as_str()),
    ]);
    let response = agent()
        .post(endpoint)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .send(body.as_bytes())
        .map_err(|e| format!("cannot reach {endpoint}: {e}"))?;
    match response.status().as_u16() {
        200 => Ok(()),
        _ => Err(format!("{endpoint} answered {}", text_of(response).0)),
    }
}

/// The models the account is offered for display, in the server's order.
pub fn models(api: &str, access_token: &str) -> Result<Vec<String>, String> {
    let url = format!("{api}/models");
    let response = agent()
        .get(&url)
        .header("Authorization", format!("Bearer {access_token}"))
        .call()
        .map_err(|e| format!("cannot reach {url}: {e}"))?;
    let (status, text) = text_of(response);
    let answer: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    let listed: Vec<String> = answer["models"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|m| m["visibility"] == "list")
        .filter_map(|m| m["slug"].as_str().map(str::to_owned))
        .collect();
    if listed.is_empty() {
        return Err(format!(
            "{url} listed no model ({status}):\n{}",
            shown(&text)
        ));
    }
    Ok(listed)
}

/// The value of `name` in a query, decoded.
fn param(query: &str, name: &str) -> Option<String> {
    let mut pairs = query.split('&').filter_map(|pair| pair.split_once('='));
    let (_, value) = pairs.find(|(key, _)| *key == name)?;
    Some(oauth::decode(value))
}

/// `urn:uuid:` and a random (version 4) UUID.
fn new_host_id() -> String {
    let hex = random_hex(16);
    let variant = ["8", "9", "a", "b"][usize::from(hex.as_bytes()[16]) % 4];
    format!(
        "urn:uuid:{}-{}-4{}-{variant}{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[13..16],
        &hex[17..20],
        &hex[20..]
    )
}

/// Waits for the browser to come back to `/auth/callback` with `state`, and gives the query
/// it came with. Anything else that connects is turned away.
fn come_back(
    listener: &TcpListener,
    state: &str,
    over: &std::sync::atomic::AtomicBool,
) -> Result<String, String> {
    let listening = |e: std::io::Error| format!("cannot listen for the browser: {e}");
    // Not blocking, so that a sign-in nobody finishes gives the port back.
    listener.set_nonblocking(true).map_err(listening)?;
    let until = Instant::now() + WAIT;
    loop {
        let accepted = listener.accept();
        // Also with a connection in hand: a tab left open is not answered.
        if over.load(std::sync::atomic::Ordering::Relaxed) {
            return Err("another sign-in was started".into());
        }
        let stream = match accepted {
            Ok((stream, _)) => stream,
            Err(e) if e.kind() == ErrorKind::WouldBlock => {
                if Instant::now() > until {
                    return Err("the browser did not come back in 10 minutes".into());
                }
                std::thread::sleep(Duration::from_millis(100));
                continue;
            }
            Err(_) => continue,
        };
        // On macOS a connection takes after its listener.
        let _ = stream.set_nonblocking(false);
        // A browser opens connections ahead of need and may send nothing on them.
        let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
        let mut lines = BufReader::new(&stream).lines().map_while(Result::ok);
        let Some(first) = lines.next() else { continue };
        // The rest of the request: closing on unread bytes would reset the connection.
        lines.find(String::is_empty);
        let target = first.split(' ').nth(1).unwrap_or("");
        let query = target.strip_prefix("/auth/callback?");
        let ours = query.filter(|q| param(q, "state").as_deref() == Some(state));
        let (status, page) = match (query, ours) {
            (_, Some(q)) if param(q, "error").is_some() || param(q, "code").is_none() => {
                (200, "ChatGPT did not sign you in. Go back to Bate.")
            }
            (_, Some(_)) => (
                200,
                "Done here. You can close this window and go back to Bate.",
            ),
            (Some(_), None) => (400, "This is not the sign-in Bate started."),
            (None, None) => (404, ""),
        };
        let _ = write!(
            &stream,
            "HTTP/1.1 {status} -\r\ncontent-type: text/plain; charset=utf-8\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{page}",
            page.len()
        );
        match (query, ours) {
            (_, Some(query)) => return Ok(query.to_owned()),
            (Some(other), None) => {
                // The error it carries, if any, is the news; a code in it is nobody's business.
                let error = param(other, "error").unwrap_or_default();
                let more = param(other, "error_description").unwrap_or_default();
                return Err(format!(
                    "the browser came back with another `state` (error: {error} {more})"
                ));
            }
            (None, None) => {}
        }
    }
}
