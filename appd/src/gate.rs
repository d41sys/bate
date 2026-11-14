//! The sign-in gate (#1), run by hand: `appd gate chatgpt [model]`. It signs in to ChatGPT the
//! way OpenAI documents for open-source local apps
//! (<https://developers.openai.com/siwc/token-sharing-open-source>), under Bate's own name,
//! sends one request and prints the streamed reply, or the exact refusal. What it prints is
//! meant to be pasted into the issue: no token, code or email is in it.

use crate::sys::{now_ms, random_hex};
use crate::{attach, oauth, paths};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::time::Duration;

const SCOPE: &str = "openid profile email offline_access resource.invoke chatgpt.tokens.use.direct";
/// The scope that lets a request count against the user's ChatGPT plan.
const PLAN: &str = "chatgpt.tokens.use.direct";
/// Most bytes read of any answer (the list of models alone is some hundred KiB).
const MAX_ANSWER: u64 = 16 << 20;
/// Most characters shown of an answer that is not a reply.
const MAX_SHOWN: usize = 2000;

/// What a sign-in leaves in `gate-chatgpt.json`, so a second run needs no second sign-in, and
/// a second sign-in no second registration.
// shortcut: the access token is not renewed; after it ends the gate signs in again. #8 renews.
#[derive(Serialize, Deserialize)]
struct Saved {
    /// This Mac, as OpenAI is told: the same for every sign-in from it.
    host_id: String,
    /// The registration OpenAI issued to this user for Bate.
    client_id: String,
    /// Empty until a code has been traded for it.
    access_token: String,
    expires_ms: u64,
}

fn agent() -> ureq::Agent {
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
fn text_of(response: ureq::http::Response<ureq::Body>) -> (String, String) {
    let id = response.headers().get("x-request-id");
    let id = id.and_then(|id| id.to_str().ok()).unwrap_or("none");
    let status = format!("{}, request id {id}", response.status().as_u16());
    let mut bytes = Vec::new();
    let mut body = response.into_body().into_reader().take(MAX_ANSWER);
    let _ = body.read_to_end(&mut bytes);
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

/// The start of `text`: enough to say what was refused.
fn shown(text: &str) -> String {
    text.chars().take(MAX_SHOWN).collect()
}

pub fn chatgpt(model: Option<String>) -> io::Result<()> {
    let file = paths::home().join("gate-chatgpt.json");
    match run(model, &file) {
        Ok(()) => {
            println!("result: a streamed reply, through response.completed");
            Ok(())
        }
        Err(why) => {
            println!("result: no reply. {why}");
            println!("(to register Bate anew, delete {})", file.display());
            Err(io::Error::other("the gate did not pass"))
        }
    }
}

fn run(model: Option<String>, file: &Path) -> Result<(), String> {
    let env = |name: &str, or: &str| std::env::var(name).unwrap_or_else(|_| or.to_owned());
    let auth = env("BATE_GATE_AUTH", "https://auth.openai.com");
    let api = env("BATE_GATE_API", "https://api.openai.com/v1");
    let old: Option<Saved> = fs::read(file)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok());
    let saved = match old {
        Some(saved) if now_ms() < saved.expires_ms => saved,
        old => sign_in(&auth, &api, old, file)?,
    };
    println!("signed in: client {}", saved.client_id);
    let model = match model {
        Some(model) => model,
        None => first_model(&api, &saved.access_token)?,
    };
    println!("model: {model}");
    reply(&api, &saved.access_token, &model)
}

fn save(file: &Path, saved: &Saved) -> Result<(), String> {
    let write = || -> io::Result<()> {
        if let Some(dir) = file.parent() {
            crate::sys::create_private_dir(dir)?;
        }
        // Whole or not at all: written beside, then moved over.
        let beside = file.with_extension("new");
        let _ = fs::remove_file(&beside);
        let mut out = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&beside)?;
        out.write_all(&serde_json::to_vec_pretty(saved)?)?;
        fs::rename(&beside, file)
    };
    write().map_err(|e| format!("cannot write {}: {e}", file.display()))
}

/// The browser's part: registration the first time, then the user's consent; back comes a
/// code, which is traded for tokens.
fn sign_in(auth: &str, api: &str, old: Option<Saved>, file: &Path) -> Result<Saved, String> {
    // The documented port; another when it is taken (only the port may differ).
    let listener = TcpListener::bind("127.0.0.1:1455")
        .or_else(|_| TcpListener::bind("127.0.0.1:0"))
        .map_err(|e| format!("cannot listen for the browser: {e}"))?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let redirect = format!("http://127.0.0.1:{port}/auth/callback");
    let (state, nonce, verifier) = (random_hex(16), random_hex(16), random_hex(32));
    let challenge = oauth::base64url(&oauth::sha256(verifier.as_bytes()));
    let host_id = old.as_ref().map_or_else(new_host_id, |s| s.host_id.clone());
    let mut fields = vec![
        ("response_type", "code"),
        ("redirect_uri", redirect.as_str()),
        ("scope", SCOPE),
        ("resource", api),
        ("state", state.as_str()),
        ("nonce", nonce.as_str()),
        ("code_challenge_method", "S256"),
        ("code_challenge", challenge.as_str()),
        ("ext_agent_host_id", host_id.as_str()),
    ];
    match &old {
        // No `id_token_hint`: the address is printed, and that token names the user.
        Some(saved) => fields.push(("client_id", saved.client_id.as_str())),
        None => fields.extend([
            ("client_id", "dynamic_agent_client"),
            ("agent_name_hint", "Bate"),
        ]),
    }
    let address = format!("{auth}/api/accounts/authorize?{}", oauth::form(&fields));
    println!("Sign in to ChatGPT in the browser. If none opened, open this address:\n{address}");
    println!(
        "(Should the browser show an error and not come back: Ctrl-C, and note what it shows.)"
    );
    let browser = std::env::var("BATE_GATE_BROWSER").unwrap_or_else(|_| "/usr/bin/open".into());
    let _ = std::process::Command::new(browser).arg(&address).status();

    let query = come_back(&listener, &state)?;
    let field = |name: &str| param(&query, name);
    if let Some(error) = field("error") {
        let more = field("error_description").unwrap_or_default();
        return Err(format!("ChatGPT refused the sign-in: {error} {more}"));
    }
    let code = field("code").ok_or("the browser came back without a code")?;
    let client_id = match (&old, field("client_id")) {
        (Some(saved), Some(id)) if id != saved.client_id => {
            return Err("the browser came back with another client id".into());
        }
        (Some(saved), _) => saved.client_id.clone(),
        (None, Some(id)) => id,
        (None, None) => return Err("registration did not finish: no client id came back".into()),
    };
    // Kept from here on: should the rest fail, the next try does not register Bate again.
    let mut saved = Saved {
        host_id,
        client_id,
        access_token: String::new(),
        expires_ms: 0,
    };
    save(file, &saved)?;

    let token_endpoint = format!("{auth}/api/accounts/oauth/token");
    let body = oauth::form(&[
        ("grant_type", "authorization_code"),
        ("client_id", &saved.client_id),
        ("code", &code),
        ("code_verifier", &verifier),
        ("redirect_uri", &redirect),
        ("resource", api),
    ]);
    let response = agent()
        .post(&token_endpoint)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .header("Accept", "application/json")
        .send(body.as_bytes())
        .map_err(|e| format!("cannot reach {token_endpoint}: {e}"))?;
    let ok = response.status() == 200;
    let (status, text) = text_of(response);
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
    let token = |name: &str| {
        let value = tokens[name].as_str().map(str::to_owned);
        value.ok_or(format!("the sign-in server gave no `{name}`"))
    };
    // shortcut: the ID token's signature is not checked: it came straight from OpenAI over
    // TLS and only its nonce is used. Check it against the JWKS when #8 keeps the identity.
    let claims = token("id_token")?
        .split('.')
        .nth(1)
        .and_then(|part| attach::base64(&part.replace('-', "+").replace('_', "/")))
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok());
    if claims.as_ref().and_then(|c| c["nonce"].as_str()) != Some(nonce.as_str()) {
        return Err("the ID token is not for this sign-in (another nonce)".into());
    }
    saved.access_token = token("access_token")?;
    let life_ms = tokens["expires_in"]
        .as_u64()
        .unwrap_or(0)
        .saturating_mul(1000);
    saved.expires_ms = now_ms() + life_ms;
    save(file, &saved)?;
    Ok(saved)
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
fn come_back(listener: &TcpListener, state: &str) -> Result<String, String> {
    for stream in listener.incoming().flatten() {
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
                (200, "ChatGPT did not sign you in. Go back to the terminal.")
            }
            (_, Some(_)) => (
                200,
                "Done here. You can close this window and go back to the terminal.",
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
    Err("stopped listening for the browser".into())
}

/// The first model the account is offered for display.
fn first_model(api: &str, access_token: &str) -> Result<String, String> {
    let url = format!("{api}/models");
    let response = agent()
        .get(&url)
        .header("Authorization", format!("Bearer {access_token}"))
        .call()
        .map_err(|e| format!("cannot reach {url}: {e}"))?;
    let (status, text) = text_of(response);
    let listed = serde_json::from_str::<Value>(&text)
        .ok()
        .and_then(|answer| {
            let models = answer["models"].as_array()?;
            let offered = models.iter().find(|m| m["visibility"] == "list")?;
            offered["slug"].as_str().map(str::to_owned)
        });
    listed.ok_or(format!(
        "{url} listed no model ({status}):\n{}",
        shown(&text)
    ))
}

/// One request; prints the reply as it streams.
fn reply(api: &str, access_token: &str, model: &str) -> Result<(), String> {
    let url = format!("{api}/responses");
    let ask = json!({
        "model": model,
        "input": [{"role": "user", "content": "Say exactly: Hello, world!"}],
        "store": false,
        "stream": true,
    });
    let response = agent()
        .post(&url)
        .header("Authorization", format!("Bearer {access_token}"))
        .header("Content-Type", "application/json")
        .send(ask.to_string().as_bytes())
        .map_err(|e| format!("cannot reach {url}: {e}"))?;
    if response.status() != 200 {
        let (status, text) = text_of(response);
        return Err(format!(
            "{url} refused the request ({status}):\n{}",
            shown(&text)
        ));
    }
    print!("reply: ");
    // What came that was no event, should nothing else come: an answer that is no stream.
    let mut other = String::new();
    for line in BufReader::new(response.into_body().into_reader()).lines() {
        let line = line.map_err(|e| format!("the stream broke: {e}"))?;
        let data = line.strip_prefix("data:").map(str::trim);
        let Some(event) = data.and_then(|data| serde_json::from_str::<Value>(data).ok()) else {
            if data.is_none() && !line.starts_with("event:") && other.len() < MAX_SHOWN {
                other.push_str(&line);
            }
            continue;
        };
        match event["type"].as_str().unwrap_or("") {
            "response.output_text.delta" => {
                print!("{}", event["delta"].as_str().unwrap_or(""));
                let _ = io::stdout().flush();
            }
            "response.completed" => {
                println!("\n[response.completed]");
                return Ok(());
            }
            "response.failed" | "response.incomplete" | "error" => {
                println!();
                return Err(format!(
                    "refused after the stream began:\n{}",
                    shown(&event.to_string())
                ));
            }
            _ => {}
        }
    }
    println!();
    Err(format!(
        "the stream ended without response.completed. It also said:\n{}",
        shown(&other)
    ))
}
