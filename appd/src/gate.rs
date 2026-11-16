//! The sign-in gate (#1), run by hand: `appd gate chatgpt [model]`. It signs in to ChatGPT the
//! way OpenAI documents for open-source local apps
//! (<https://developers.openai.com/siwc/token-sharing-open-source>), under Bate's own name,
//! sends one request and prints the streamed reply, or the exact refusal. What it prints is
//! meant to be pasted into the issue: no token, code or email is in it.

use crate::chatgpt::{self, Saved, agent, shown, text_of};
use crate::paths;
use crate::sys::now_ms;
use serde_json::{Value, json};
use std::fs;
use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

/// Most characters shown of an answer that is not a reply.
const MAX_SHOWN: usize = 2000;

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

// shortcut: the access token is not renewed here; after it ends the gate signs in again. The
// service renews (#8).
fn run(model: Option<String>, file: &Path) -> Result<(), String> {
    let (auth, api) = (chatgpt::auth(), chatgpt::api());
    let old: Option<Saved> = fs::read(file)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok());
    let saved = match old {
        Some(saved) if now_ms() < saved.expires_ms => saved,
        old => sign_in(&auth, &api, old.as_ref(), file)?,
    };
    println!("signed in: client {}", saved.client_id);
    let model = match model {
        Some(model) => model,
        None => chatgpt::models(&api, &saved.access_token)?.remove(0),
    };
    println!("model: {model}");
    reply(&api, &saved.access_token, &model)
}

/// The browser's part: registration the first time, then the user's consent; back comes a
/// code, which is traded for tokens.
fn sign_in(auth: &str, api: &str, old: Option<&Saved>, file: &Path) -> Result<Saved, String> {
    let (pending, address) = chatgpt::begin(auth, api, old)?;
    println!("Sign in to ChatGPT in the browser. If none opened, open this address:\n{address}");
    println!(
        "(Should the browser show an error and not come back: Ctrl-C, and note what it shows.)"
    );
    let browser = std::env::var("BATE_GATE_BROWSER").unwrap_or_else(|_| "/usr/bin/open".into());
    let _ = std::process::Command::new(browser).arg(&address).status();
    let saved = chatgpt::finish(pending, |saved| save(file, saved))?;
    save(file, &saved)?;
    Ok(saved)
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
