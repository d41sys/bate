//! `appd gate chatgpt` against a stand-in for OpenAI's sign-in and Responses servers.

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Child, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

/// OpenAI's part. `seen`: one line per request, `METHOD path | authorization | body`.
struct OpenAi {
    base: String,
    seen: Arc<Mutex<Vec<String>>>,
    /// The nonce the next ID token carries.
    nonce: Arc<Mutex<String>>,
    /// `/v1/responses` answers 403.
    refuses: Arc<AtomicBool>,
    /// The user did not let Bate use the ChatGPT plan: the tokens are for the identity only.
    no_plan: Arc<AtomicBool>,
}

fn base64url(data: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    for chunk in data.chunks(3) {
        let n = chunk.iter().fold(0u32, |n, &b| (n << 8) | u32::from(b)) << (8 * (3 - chunk.len()));
        for i in 0..=chunk.len() {
            out.push(ALPHABET[(n >> (18 - 6 * i)) as usize & 63] as char);
        }
    }
    out
}

const REFUSAL: &str =
    r#"{"error":{"code":"subscription_sharing_usage_unavailable","message":"Not for this plan."}}"#;

impl OpenAi {
    fn start() -> OpenAi {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let (seen, nonce, refuses, no_plan) = (
            Arc::new(Mutex::new(Vec::new())),
            Arc::new(Mutex::new(String::new())),
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicBool::new(false)),
        );
        let (log, next_nonce, refuse) = (seen.clone(), nonce.clone(), refuses.clone());
        let identity_only = no_plan.clone();
        std::thread::spawn(move || {
            for mut stream in listener.incoming().flatten() {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let (mut first, mut auth, mut length) = (String::new(), String::new(), 0);
                reader.read_line(&mut first).unwrap();
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if line == "\r\n" || line.is_empty() {
                        break;
                    }
                    let lower = line.to_ascii_lowercase();
                    if let Some(v) = lower.strip_prefix("content-length:") {
                        length = v.trim().parse().unwrap();
                    }
                    if lower.starts_with("authorization:") {
                        auth = line["authorization:".len()..].trim().to_owned();
                    }
                }
                let mut body = vec![0; length];
                reader.read_exact(&mut body).unwrap();
                let first = first.trim_end().trim_end_matches(" HTTP/1.1").to_owned();
                let body = String::from_utf8(body).unwrap();
                log.lock()
                    .unwrap()
                    .push(format!("{first} | {auth} | {body}"));
                let (status, kind, answer) = match first.as_str() {
                    "POST /api/accounts/oauth/token" => {
                        let claims = json!({"sub": "user-1", "nonce": *next_nonce.lock().unwrap()});
                        let id_token =
                            format!("e30.{}.sig", base64url(claims.to_string().as_bytes()));
                        let scope = if identity_only.load(Ordering::SeqCst) {
                            "email openid profile"
                        } else {
                            "chatgpt.tokens.use.direct email offline_access openid profile resource.invoke"
                        };
                        let tokens = json!({
                            "access_token": "at-1", "refresh_token": "rt-1", "id_token": id_token,
                            "token_type": "Bearer", "expires_in": 3600,
                            "scope": scope,
                        });
                        (200, "application/json", tokens.to_string())
                    }
                    "GET /v1/models" => {
                        // As long as the real list: some hundred KiB before the first shown.
                        let mut models = vec![json!({"slug": "hidden", "visibility": "hide",
                            "base_instructions": "x".repeat(300_000)})];
                        models.push(json!({"slug": "gpt-test", "display_name": "GPT Test", "visibility": "list"}));
                        let models = json!({"models": models});
                        (200, "application/json", models.to_string())
                    }
                    "POST /v1/responses" if refuse.load(Ordering::SeqCst) => (
                        403,
                        "application/json\r\nx-request-id: req_77",
                        REFUSAL.to_owned(),
                    ),
                    "POST /v1/responses" => {
                        let events = [
                            json!({"type": "response.created"}),
                            json!({"type": "response.output_text.delta", "delta": "Hello, "}),
                            json!({"type": "response.output_text.delta", "delta": "world!"}),
                            json!({"type": "response.completed"}),
                        ];
                        let stream: String = events
                            .iter()
                            .map(|e| {
                                format!("event: {}\ndata: {e}\n\n", e["type"].as_str().unwrap())
                            })
                            .collect();
                        (200, "text/event-stream", stream)
                    }
                    _ => (404, "text/plain", String::new()),
                };
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status} -\r\ncontent-type: {kind}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{answer}",
                    answer.len()
                );
            }
        });
        OpenAi {
            base,
            seen,
            nonce,
            refuses,
            no_plan,
        }
    }

    fn seen(&self, start: &str) -> Vec<String> {
        let seen = self.seen.lock().unwrap();
        seen.iter()
            .filter(|line| line.starts_with(start))
            .cloned()
            .collect()
    }
}

struct Home(PathBuf);

impl Drop for Home {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn home(name: &str) -> Home {
    let dir = std::env::temp_dir().join(format!("appd-gate-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    Home(dir)
}

fn gate(home: &Home, openai: &OpenAi) -> (Child, BufReader<ChildStdout>) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_appd"))
        .args(["gate", "chatgpt"])
        .env("APPD_HOME", &home.0)
        .env("BATE_CHATGPT_AUTH", &openai.base)
        .env("BATE_CHATGPT_API", format!("{}/v1", openai.base))
        // No browser in a test: the test is the browser.
        .env("BATE_GATE_BROWSER", "/usr/bin/true")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let out = BufReader::new(child.stdout.take().unwrap());
    (child, out)
}

/// The value of `name` in the query of `url`, decoded.
fn param(url: &str, name: &str) -> Option<String> {
    let (_, query) = url.split_once('?')?;
    let (_, value) = query
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .find(|(key, _)| *key == name)?;
    let bytes = value.as_bytes();
    let (mut out, mut i) = (Vec::new(), 0);
    while i < bytes.len() {
        if bytes[i] == b'%' {
            out.push(u8::from_str_radix(&value[i + 1..i + 3], 16).unwrap());
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    Some(String::from_utf8(out).unwrap())
}

/// The address the gate asks the user to open.
fn address(out: &mut BufReader<ChildStdout>, openai: &OpenAi) -> String {
    loop {
        let mut line = String::new();
        assert_ne!(
            out.read_line(&mut line).unwrap(),
            0,
            "the gate gave no address"
        );
        if line.starts_with(&openai.base) {
            return line.trim().to_owned();
        }
    }
}

fn finish(mut child: Child, mut out: BufReader<ChildStdout>) -> (bool, String) {
    let mut rest = String::new();
    out.read_to_string(&mut rest).unwrap();
    (child.wait().unwrap().success(), rest)
}

fn come_back(url: &str, query: &str) -> u16 {
    let config = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .build();
    let agent: ureq::Agent = config.into();
    let back = format!("{}?{query}", param(url, "redirect_uri").unwrap());
    agent.get(&back).call().unwrap().status().as_u16()
}

#[test]
fn the_gate_registers_signs_in_and_prints_a_streamed_reply_or_the_exact_refusal() {
    let (home, openai) = (home("reply"), OpenAi::start());
    let (child, mut out) = gate(&home, &openai);
    let url = address(&mut out, &openai);

    // The documented first-time registration, under Bate's own name.
    assert!(
        url.starts_with(&format!("{}/api/accounts/authorize?", openai.base)),
        "{url}"
    );
    let p = |name| param(&url, name).unwrap_or_else(|| panic!("no `{name}` in {url}"));
    assert_eq!(p("client_id"), "dynamic_agent_client");
    assert_eq!(p("agent_name_hint"), "Bate");
    assert_eq!(p("response_type"), "code");
    assert_eq!(p("code_challenge_method"), "S256");
    assert_eq!(p("resource"), format!("{}/v1", openai.base));
    assert_eq!(
        p("scope"),
        "openid profile email offline_access resource.invoke chatgpt.tokens.use.direct"
    );
    // A random (version 4) UUID.
    let host_id = p("ext_agent_host_id");
    let (version, variant) = (&host_id[23..24], &host_id[28..29]);
    assert!(
        host_id.starts_with("urn:uuid:")
            && host_id.len() == 45
            && version == "4"
            && "89ab".contains(variant),
        "{host_id}"
    );
    let redirect = p("redirect_uri");
    assert!(
        redirect.starts_with("http://127.0.0.1:") && redirect.ends_with("/auth/callback"),
        "{redirect}"
    );
    assert_eq!(param(&url, "id_token_hint"), None);

    *openai.nonce.lock().unwrap() = p("nonce");
    let query = format!("code=c0de&state={}&client_id=oaiapp_1", p("state"));
    assert_eq!(come_back(&url, &query), 200);
    let (ok, rest) = finish(child, out);
    assert!(ok, "{rest}");
    assert!(rest.contains("Hello, world!"), "{rest}");
    assert!(rest.contains("response.completed"), "{rest}");

    // The code was traded with the issued client id, not the registration one.
    let token = &openai.seen("POST /api/accounts/oauth/token")[0];
    for field in [
        "grant_type=authorization_code",
        "code=c0de",
        "client_id=oaiapp_1",
        "redirect_uri=http%3A%2F%2F127.0.0.1",
    ] {
        assert!(token.contains(field), "{field}: {token}");
    }
    // The verifier is the one whose hash the browser was sent with.
    let verifier = param(
        &format!("?{}", token.rsplit(" | ").next().unwrap()),
        "code_verifier",
    )
    .unwrap();
    let hash = ring::digest::digest(&ring::digest::SHA256, verifier.as_bytes());
    assert_eq!(p("code_challenge"), base64url(hash.as_ref()));
    // Nothing printed is a secret: it is pasted into the issue.
    for secret in ["at-1", "rt-1", "c0de", verifier.as_str(), "user-1"] {
        assert!(
            !url.contains(secret) && !rest.contains(secret),
            "{secret}: {rest}"
        );
    }
    let asked = &openai.seen("POST /v1/responses")[0];
    let (head, body) = asked.rsplit_once(" | ").unwrap();
    assert!(head.ends_with("| Bearer at-1"), "{head}");
    let body: Value = serde_json::from_str(body).unwrap();
    assert_eq!(
        (&body["model"], &body["store"], &body["stream"]),
        (&json!("gpt-test"), &json!(false), &json!(true))
    );

    // What was signed in is kept, for the owner only, and used again: no second sign-in.
    let file = home.0.join("gate-chatgpt.json");
    assert_eq!(
        std::fs::metadata(&file).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let kept: Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    assert_eq!(
        (&kept["client_id"], &kept["host_id"]),
        (&json!("oaiapp_1"), &json!(host_id))
    );

    openai.refuses.store(true, Ordering::SeqCst);
    let (child, out) = gate(&home, &openai);
    let (ok, rest) = finish(child, out);
    assert!(!ok, "{rest}");
    assert!(
        rest.contains("403") && rest.contains("req_77") && rest.contains(REFUSAL),
        "{rest}"
    );
    assert_eq!(openai.seen("POST /api/accounts/oauth/token").len(), 1);

    // The token has ended: the same registration and the same Mac sign in again, and the
    // address, which is printed, names nobody.
    let mut ended = kept.clone();
    ended["expires_ms"] = json!(0);
    std::fs::write(&file, ended.to_string()).unwrap();
    let (child, mut out) = gate(&home, &openai);
    let url = address(&mut out, &openai);
    let p = |name| param(&url, name);
    assert_eq!(p("client_id").as_deref(), Some("oaiapp_1"));
    assert_eq!(p("ext_agent_host_id"), Some(host_id));
    assert_eq!((p("agent_name_hint"), p("id_token_hint")), (None, None));
    // An answer for another registration is not this one's.
    let query = format!("code=c0de&state={}&client_id=oaiapp_2", p("state").unwrap());
    come_back(&url, &query);
    let (ok, rest) = finish(child, out);
    assert!(!ok && rest.contains("another client id"), "{rest}");
    assert_eq!(openai.seen("POST /api/accounts/oauth/token").len(), 1);
    // It may come back without the client id.
    let (child, mut out) = gate(&home, &openai);
    let url = address(&mut out, &openai);
    *openai.nonce.lock().unwrap() = param(&url, "nonce").unwrap();
    let state = param(&url, "state").unwrap();
    come_back(&url, &format!("code=c0de2&state={state}"));
    let (_, rest) = finish(child, out);
    assert!(rest.contains("signed in: client oaiapp_1"), "{rest}");
    let token = &openai.seen("POST /api/accounts/oauth/token")[1];
    assert!(
        token.contains("client_id=oaiapp_1") && token.contains("code=c0de2"),
        "{token}"
    );
}

#[test]
fn the_gate_takes_no_code_from_an_answer_that_is_not_to_its_own_request() {
    let (home, openai) = (home("state"), OpenAi::start());
    let (child, mut out) = gate(&home, &openai);
    let url = address(&mut out, &openai);
    *openai.nonce.lock().unwrap() = param(&url, "nonce").unwrap();
    assert_eq!(
        come_back(&url, "code=c0de&state=another&client_id=oaiapp_1"),
        400
    );
    let (ok, rest) = finish(child, out);
    assert!(!ok, "{rest}");
    assert_eq!(openai.seen("POST").len(), 0, "{rest}");

    // Nor tokens whose ID token answers another request.
    let (child, mut out) = gate(&home, &openai);
    let url = address(&mut out, &openai);
    *openai.nonce.lock().unwrap() = "another".into();
    let query = format!(
        "code=c0de&state={}&client_id=oaiapp_1",
        param(&url, "state").unwrap()
    );
    come_back(&url, &query);
    let (ok, rest) = finish(child, out);
    assert!(!ok, "{rest}");
    assert_eq!(openai.seen("POST /v1/responses").len(), 0, "{rest}");

    // Nor tokens that are not for the ChatGPT plan.
    openai.no_plan.store(true, Ordering::SeqCst);
    let (child, mut out) = gate(&home, &openai);
    let url = address(&mut out, &openai);
    *openai.nonce.lock().unwrap() = param(&url, "nonce").unwrap();
    let query = format!(
        "code=c0de&state={}&client_id=oaiapp_1",
        param(&url, "state").unwrap()
    );
    come_back(&url, &query);
    let (ok, rest) = finish(child, out);
    assert!(!ok && rest.contains("was not granted"), "{rest}");
    assert_eq!(openai.seen("POST /v1/responses").len(), 0, "{rest}");
    // What was registered is kept, without a token: the next try does not register again.
    let kept = std::fs::read(home.0.join("gate-chatgpt.json")).unwrap();
    let kept: Value = serde_json::from_slice(&kept).unwrap();
    assert_eq!(
        (
            &kept["client_id"],
            &kept["access_token"],
            &kept["expires_ms"]
        ),
        (&json!("oaiapp_1"), &json!(""), &json!(0))
    );
    let (child, mut out) = gate(&home, &openai);
    let url = address(&mut out, &openai);
    assert_eq!(param(&url, "client_id").as_deref(), Some("oaiapp_1"));
    // A refusal is shown, also one that names no state; a code in it is not.
    come_back(
        &url,
        "error=access_denied&error_description=No+plan&code=c0de9",
    );
    let (ok, rest) = finish(child, out);
    assert!(!ok && rest.contains("access_denied No plan"), "{rest}");
    assert!(!rest.contains("c0de9"), "{rest}");
}
