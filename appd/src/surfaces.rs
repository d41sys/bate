//! Surfaces: a table, a tree or a code view that a project's program fills and the window draws
//! beside the conversation. A surface is an MCP resource of the program whose `mimeType` is one
//! of Bate's own (`TABLE`, `TREE`, `CODE`); the service is the one MCP client of the program
//! here: it lists and reads the resources, subscribes to their changes, and calls the tools
//! that the person's actions name. What reaches the window is data, cut to a hard size, and
//! the window shows it as text.

use crate::core::Msg;
use crate::project_server;
use serde_json::{Value, json};
use std::collections::HashSet;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

const TABLE: &str = "application/vnd.bate.table+json";
const TREE: &str = "application/vnd.bate.tree+json";
const CODE: &str = "application/vnd.bate.code+json";

/// Most surfaces listed for a project, and most pages of the program's list that are read.
const MAX_SURFACES: usize = 100;
const MAX_PAGES: usize = 10;
/// Most rows, nodes or lines in one part, and the part that is sent when none is asked for.
pub const MAX_PART: usize = 500;
const PART: usize = 100;
/// Most bytes of text in one part: items are left out from the one that goes over.
const MAX_PART_BYTES: usize = 512 * 1024;
/// Most bytes of an answer of the program that are read.
const MAX_ANSWER: u64 = 16 << 20;
/// Longest cell, label and line, in bytes; the rest is cut.
const MAX_CELL: usize = 1000;
const MAX_LINE: usize = 2000;
const MAX_COLUMNS: usize = 20;
const MAX_ACTIONS: usize = 20;
/// Deepest level of a tree's node.
const MAX_DEPTH: u64 = 64;
/// How long the program may take over one call.
const CALL: Duration = Duration::from_secs(30);

/// The service's MCP session with one program: made when first needed, and as long as it runs.
pub struct Link {
    socket: PathBuf,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    session: Option<Session>,
    /// The stream of the program's notifications is being read.
    listening: bool,
    /// Resources the program was asked to tell about.
    subscribed: HashSet<String>,
}

struct Session {
    id: Option<String>,
    resources: bool,
    subscribe: bool,
}

/// What the program answered: `Err` is its own error, or why there was no answer.
type Answered = Result<Value, String>;

/// A call of the program, made off the core's thread.
pub type Job = Box<dyn FnOnce(&Link) -> Answered + Send>;

impl Link {
    pub fn new(socket: PathBuf) -> Link {
        Link {
            socket,
            state: Mutex::default(),
        }
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// One HTTP request to the program, as 1.0: its answer cannot be chunked, and ends with
    /// the connection. Gives the stream after the answer's head, and the head.
    fn send(
        &self,
        method: &str,
        session: Option<&str>,
        body: &[u8],
    ) -> Result<(BufReader<std::os::unix::net::UnixStream>, Head), String> {
        let down = |e: std::io::Error| format!("cannot reach the program: {e}");
        let mut stream = project_server::connect(&self.socket).map_err(down)?;
        let mut head = format!(
            "{method} /mcp HTTP/1.0\r\nhost: localhost\r\naccept: application/json, text/event-stream\r\n"
        );
        if method == "POST" {
            head.push_str(&format!(
                "content-type: application/json\r\ncontent-length: {}\r\n",
                body.len()
            ));
        }
        if let Some(id) = session {
            head.push_str(&format!("mcp-session-id: {id}\r\n"));
        }
        head.push_str("\r\n");
        // A stream (GET) may be quiet as long as it likes.
        if method == "POST" {
            stream.set_read_timeout(Some(CALL)).map_err(down)?;
        }
        stream
            .write_all(head.as_bytes())
            .and_then(|()| stream.write_all(body))
            .map_err(down)?;
        let mut reader = BufReader::new(stream);
        let mut status = String::new();
        reader.read_line(&mut status).map_err(down)?;
        let code = status
            .split(' ')
            .nth(1)
            .and_then(|c| c.parse().ok())
            .ok_or("the program did not answer in HTTP")?;
        let mut answer = Head {
            status: code,
            session: None,
            stream: false,
        };
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).map_err(down)?;
            let Some((name, value)) = line.trim_end().split_once(':') else {
                break;
            };
            let value = value.trim();
            match name.to_ascii_lowercase().as_str() {
                "mcp-session-id" => answer.session = Some(value.to_owned()),
                "content-type" => answer.stream = value.starts_with("text/event-stream"),
                _ => {}
            }
        }
        Ok((reader, answer))
    }

    /// A JSON-RPC request and its result. The answer may be one JSON message or a stream of
    /// events that holds it.
    fn rpc(&self, session: Option<&str>, method: &str, params: Value) -> Answered {
        let body = json!({"jsonrpc": "2.0", "id": 2, "method": method, "params": params});
        let (mut reader, head) = self.send("POST", session, body.to_string().as_bytes())?;
        let mut text = String::new();
        let read = (&mut reader).take(MAX_ANSWER).read_to_string(&mut text);
        read.map_err(|e| format!("the program's answer is not text: {e}"))?;
        if head.status >= 300 {
            return Err(format!("the program answered {}", head.status));
        }
        let reply = if head.stream {
            events(&text)
                .into_iter()
                .find(|m| m.get("id") == Some(&json!(2)))
                .ok_or("the program's answer holds no reply")?
        } else {
            serde_json::from_str(&text).map_err(|e| format!("the program's answer: {e}"))?
        };
        if let Some(error) = reply.get("error") {
            let why = error.get("message").and_then(Value::as_str);
            return Err(why.unwrap_or("the program refused").to_owned());
        }
        Ok(reply.get("result").cloned().unwrap_or(Value::Null))
    }

    /// The session, started on first use: `initialize`, and what the program says it offers.
    fn session(&self) -> Result<(Option<String>, bool, bool), String> {
        let mut state = self.state();
        if state.session.is_none() {
            let init = json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
                "protocolVersion": "2025-03-26",
                "capabilities": {},
                "clientInfo": {"name": "bate", "version": env!("CARGO_PKG_VERSION")},
            }});
            let (mut reader, head) = self.send("POST", None, init.to_string().as_bytes())?;
            let mut text = String::new();
            let read = (&mut reader).take(MAX_ANSWER).read_to_string(&mut text);
            read.map_err(|e| format!("the program's answer is not text: {e}"))?;
            let reply = if head.stream {
                events(&text).into_iter().next().unwrap_or(Value::Null)
            } else {
                serde_json::from_str(&text).unwrap_or(Value::Null)
            };
            let caps = &reply["result"]["capabilities"]["resources"];
            let session = Session {
                id: head.session,
                resources: caps.is_object(),
                subscribe: caps["subscribe"] == json!(true),
            };
            let done = json!({"jsonrpc": "2.0", "method": "notifications/initialized"});
            self.send("POST", session.id.as_deref(), done.to_string().as_bytes())?;
            state.session = Some(session);
        }
        let s = state.session.as_ref().expect("just made");
        Ok((s.id.clone(), s.resources, s.subscribe))
    }

    fn call(&self, method: &str, params: Value) -> Answered {
        let (id, resources, _) = self.session()?;
        if !resources {
            return Err("the program offers no surfaces".into());
        }
        self.rpc(id.as_deref(), method, params)
    }

    /// The surfaces the program offers: its resources of Bate's own content types.
    pub fn list(&self) -> Answered {
        if !self.session()?.1 {
            return Ok(json!({"surfaces": []}));
        }
        let (mut found, mut cursor) = (Vec::new(), None::<String>);
        for _ in 0..MAX_PAGES {
            let params = cursor.as_ref().map_or(json!({}), |c| json!({"cursor": c}));
            let page = self.call("resources/list", params)?;
            for r in page["resources"].as_array().into_iter().flatten() {
                let kind = r["mimeType"].as_str().and_then(kind_of);
                let uri = r["uri"]
                    .as_str()
                    .filter(|u| !u.is_empty() && u.len() <= 500);
                if let (Some(kind), Some(uri)) = (kind, uri) {
                    let name = r["name"].as_str().unwrap_or(uri);
                    let mut s = json!({"uri": uri, "name": cut(name, 200), "kind": kind});
                    if let Some(d) = r["description"].as_str() {
                        s["description"] = json!(cut(d, 500));
                    }
                    found.push(s);
                }
            }
            cursor = page["nextCursor"].as_str().map(str::to_owned);
            if cursor.is_none() || found.len() >= MAX_SURFACES {
                break;
            }
        }
        found.truncate(MAX_SURFACES);
        Ok(json!({"surfaces": found}))
    }

    /// A part of a surface's content, from `offset`, at most `limit` items (and never more
    /// than `MAX_PART`): the program is asked for that part and cannot make it larger. Also
    /// subscribes to the surface's changes, once.
    pub fn read(&self, uri: &str, offset: Option<u64>, limit: Option<u64>) -> Answered {
        let offset = offset.map_or(0, |o| o as usize);
        let limit = limit.map_or(PART, |l| l as usize).clamp(1, MAX_PART);
        let part = json!({"bate/part": {"offset": offset, "limit": limit}});
        let read = self.call("resources/read", json!({"uri": uri, "_meta": part}))?;
        let content = &read["contents"][0];
        let kind = content["mimeType"].as_str().and_then(kind_of);
        let kind = kind.ok_or("that is not a surface")?;
        let text = content["text"].as_str().ok_or("the content is not text")?;
        let doc = serde_json::from_str(text).map_err(|e| format!("the content: {e}"))?;
        let part = cut_part(kind, &doc, offset, limit)?;
        let (_, _, subscribe) = self.session()?;
        if subscribe && self.state().subscribed.insert(uri.to_owned()) {
            // A program that cannot is read again by the window after its own actions.
            let _ = self.call("resources/subscribe", json!({"uri": uri}));
        }
        Ok(part)
    }

    /// The person's action: the tool `tool` of the program, if the surface offers it, with the
    /// surface and what was chosen (`target`: a row's or node's id, or a line's number).
    pub fn act(&self, uri: &str, tool: &str, target: Option<&str>) -> Answered {
        let now = self.read(uri, Some(0), Some(1))?;
        let offered = ["select", "open"]
            .iter()
            .filter_map(|k| now[k].as_str())
            .chain(
                now["actions"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|a| a["tool"].as_str()),
            )
            .any(|t| t == tool);
        if !offered {
            return Err(format!("the surface does not offer `{tool}`"));
        }
        let mut arguments = json!({"uri": uri});
        if let Some(target) = target {
            arguments["target"] = json!(cut(target, 200));
        }
        let done = self.call("tools/call", json!({"name": tool, "arguments": arguments}))?;
        let said: Vec<&str> = done["content"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|c| c["text"].as_str())
            .collect();
        Ok(json!({"ok": done["isError"] != json!(true), "text": cut(&said.join("\n"), 2000)}))
    }

    /// Starts reading the program's notifications, once: what changed goes to the core as
    /// `Msg::SurfaceNews` for project `id`'s program of `generation`.
    pub fn listen(self: &Arc<Self>, id: &str, generation: u64, tx: &Sender<Msg>) {
        if std::mem::replace(&mut self.state().listening, true) {
            return;
        }
        let (link, id, tx) = (self.clone(), id.to_owned(), tx.clone());
        thread::spawn(move || link.stream(&id, generation, &tx));
    }

    /// Ends when the program cannot be reached or offers no stream.
    fn stream(&self, id: &str, generation: u64, tx: &Sender<Msg>) {
        loop {
            let Ok((session, ..)) = self.session() else {
                return;
            };
            let Ok((reader, head)) = self.send("GET", session.as_deref(), &[]) else {
                return;
            };
            if head.status != 200 || !head.stream {
                return;
            }
            let mut data = String::new();
            for line in reader.lines().map_while(Result::ok) {
                if let Some(rest) = line.strip_prefix("data:") {
                    data.push_str(rest.trim_start());
                } else if line.is_empty() && !data.is_empty() {
                    let news = news_of(&std::mem::take(&mut data));
                    let (id, generation) = (id.to_owned(), generation);
                    if let Some(uri) = news
                        && tx
                            .send(Msg::SurfaceNews {
                                id,
                                generation,
                                uri,
                            })
                            .is_err()
                    {
                        return;
                    }
                }
            }
            // The stream ended: tried again; a program that is gone cannot be reached, which ends this.
            thread::sleep(Duration::from_secs(1));
        }
    }
}

struct Head {
    status: u16,
    session: Option<String>,
    /// The answer is `text/event-stream`.
    stream: bool,
}

/// The JSON messages of server-sent events.
fn events(text: &str) -> Vec<Value> {
    let mut out = Vec::new();
    let mut data = String::new();
    for line in text.lines().chain(std::iter::once("")) {
        if let Some(rest) = line.strip_prefix("data:") {
            data.push_str(rest.trim_start());
        } else if line.is_empty() && !data.is_empty() {
            out.extend(serde_json::from_str(&std::mem::take(&mut data)));
        }
    }
    out
}

fn kind_of(mime: &str) -> Option<&'static str> {
    match mime {
        TABLE => Some("table"),
        TREE => Some("tree"),
        CODE => Some("code"),
        _ => None,
    }
}

/// Shortens `text` to at most `max` bytes, at a character's edge.
fn cut(text: &str, max: usize) -> String {
    let mut end = text.len().min(max);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}

/// A value as the text it is shown as.
fn text(v: &Value, max: usize) -> String {
    match v {
        Value::String(s) => cut(s, max),
        Value::Null => String::new(),
        other => cut(&other.to_string(), max),
    }
}

/// What a notification says: `Some(None)`: the list changed; `Some(Some(uri))`: that
/// resource changed; `None`: nothing for the window.
fn news_of(data: &str) -> Option<Option<String>> {
    let m: Value = serde_json::from_str(data).ok()?;
    match m["method"].as_str()? {
        "notifications/resources/updated" => Some(Some(m["params"]["uri"].as_str()?.to_owned())),
        "notifications/resources/list_changed" => Some(None),
        _ => None,
    }
}

/// The part of a program's content that the window gets: validated, in the window's own
/// shape, and cut to `limit` items and `MAX_PART_BYTES`.
///
/// A program that sends `offset` says where its items begin in the whole and is taken at its
/// word; one that does not has sent the whole from its start, and the part is cut from it.
fn cut_part(kind: &str, doc: &Value, offset: usize, limit: usize) -> Answered {
    let key = match kind {
        "table" => "rows",
        "tree" => "nodes",
        _ => "lines",
    };
    let all = doc[key]
        .as_array()
        .ok_or_else(|| format!("the content has no list `{key}`"))?;
    let (first, skip) = match doc["offset"].as_u64() {
        Some(o) => (o as usize, 0),
        None => (offset.min(all.len()), offset.min(all.len())),
    };
    let mut bytes = 0;
    let mut items = Vec::new();
    for (i, v) in all.iter().skip(skip).take(limit).enumerate() {
        let n = first + i;
        let item = match kind {
            "table" => {
                let id = v["id"].as_str().map_or(n.to_string(), |s| cut(s, 200));
                let cells: Vec<String> = v["cells"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .take(MAX_COLUMNS)
                    .map(|c| text(c, MAX_CELL))
                    .collect();
                bytes += cells.iter().map(String::len).sum::<usize>();
                json!({"id": id, "cells": cells})
            }
            "tree" => {
                let id = v["id"].as_str().map_or(n.to_string(), |s| cut(s, 200));
                let label = text(&v["label"], MAX_CELL);
                bytes += label.len();
                let depth = v["depth"].as_u64().unwrap_or(0).min(MAX_DEPTH);
                json!({"id": id, "label": label, "depth": depth})
            }
            _ => {
                let line = text(v, MAX_LINE);
                bytes += line.len();
                json!(line)
            }
        };
        bytes += 32;
        if bytes > MAX_PART_BYTES && !items.is_empty() {
            break;
        }
        items.push(item);
    }
    let total = doc["total"]
        .as_u64()
        .map_or(first + all.len() - skip, |t| t as usize);
    let mut out = json!({
        "kind": kind,
        "offset": first,
        "total": total.max(first + items.len()),
        key: items,
    });
    match kind {
        "table" => {
            let columns = doc["columns"].as_array().into_iter().flatten();
            let columns: Vec<String> = columns.take(MAX_COLUMNS).map(|c| text(c, 200)).collect();
            out["columns"] = json!(columns);
        }
        "code" => out["language"] = json!(text(&doc["language"], 40)),
        _ => {}
    }
    for k in ["select", "open"] {
        if let Some(tool) = doc[k].as_str().filter(|t| !t.is_empty()) {
            out[k] = json!(cut(tool, 128));
        }
    }
    let actions: Vec<Value> = doc["actions"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|a| {
            let tool = a["tool"].as_str().filter(|t| !t.is_empty())?;
            let label = a["label"].as_str().unwrap_or(tool);
            Some(json!({"tool": cut(tool, 128), "label": cut(label, 80)}))
        })
        .take(MAX_ACTIONS)
        .collect();
    out["actions"] = json!(actions);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_program_that_sends_everything_is_cut_to_the_part_asked_for() {
        let rows: Vec<Value> = (0..2000).map(|n| json!({"cells": [n]})).collect();
        let doc = json!({"columns": ["n"], "rows": rows});
        let part = cut_part("table", &doc, 10, 3).unwrap();
        assert_eq!(part["offset"], 10);
        assert_eq!(part["total"], 2000);
        assert_eq!(part["rows"][0], json!({"id": "10", "cells": ["10"]}));
        assert_eq!(part["rows"].as_array().unwrap().len(), 3);
    }

    #[test]
    fn a_program_that_gives_its_offset_is_taken_at_its_word_but_not_past_the_limit() {
        let doc = json!({"lines": ["a", "b", "c"], "offset": 40, "total": 90});
        let part = cut_part("code", &doc, 40, 2).unwrap();
        assert_eq!(
            (part["offset"].clone(), part["total"].clone()),
            (json!(40), json!(90))
        );
        assert_eq!(part["lines"], json!(["a", "b"]));
    }

    #[test]
    fn content_is_bounded_in_bytes_and_stays_text() {
        let long = "é".repeat(5000);
        let doc = json!({"lines": [long, {"nested": [1, null]}, 3, true]});
        let part = cut_part("code", &doc, 0, 10).unwrap();
        assert_eq!(part["lines"][0].as_str().unwrap().len(), 2000);
        assert_eq!(part["lines"][1], r#"{"nested":[1,null]}"#);
        assert_eq!(part["lines"][2], "3");
        let wide = json!({"lines": vec!["x".repeat(2000); 500]});
        let part = cut_part("code", &wide, 0, 500).unwrap();
        assert!(part["lines"].as_array().unwrap().len() < 300);
        assert_eq!(part["total"], 500);
    }

    #[test]
    fn a_list_that_is_not_there_is_refused() {
        assert!(cut_part("tree", &json!({"rows": []}), 0, 10).is_err());
    }
}
