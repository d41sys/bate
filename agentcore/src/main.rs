//! `agentcore`: Bate's own engine. Speaks the engine protocol (`PROTOCOL.md`) on
//! stdin/stdout in the bot's folder; one turn at a time runs on its own thread.

mod model;
mod session;
mod tools;
mod turn;

use proto::{Event, MAX_EVENT_FRAME, MAX_OP_FRAME, Op, Request};
use session::Session;
use std::io::{self, BufReader};
use std::path::PathBuf;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use tools::{Control, lock};

/// Event output, shared by the operation loop and the turn thread.
#[derive(Clone)]
pub struct Out(Arc<Mutex<io::Stdout>>);

impl Out {
    pub fn send(&self, event: &Event) {
        if let Err(e) = proto::write_frame(&mut *lock(&self.0), event, MAX_EVENT_FRAME) {
            // The service reads our output; without it there is nothing left to do.
            eprintln!("cannot send an event: {e}");
            std::process::exit(1);
        }
    }
}

enum Input {
    Op(Request),
    /// The turn thread is done and hands the conversation back.
    TurnDone(Session),
    Closed,
}

/// A turn in progress: its conversation is on the turn thread until `TurnDone`.
struct Running {
    control: Arc<Control>,
    answers: Sender<(String, String)>,
    steer: Sender<String>,
}

struct Engine {
    out: Out,
    inputs: Sender<Input>,
    dir: PathBuf,
    session: Option<Session>,
    running: Option<Running>,
    model: Option<String>,
    effort: Option<String>,
}

fn main() {
    let out = Out(Arc::new(Mutex::new(io::stdout())));
    out.send(&Event::Ready {
        protocol: proto::PROTOCOL,
    });
    let (inputs, rx) = mpsc::channel();
    let reader = inputs.clone();
    thread::spawn(move || {
        let mut stdin = BufReader::new(io::stdin());
        let mut buf = Vec::new();
        loop {
            match proto::read_frame::<Request>(&mut stdin, MAX_OP_FRAME, &mut buf) {
                Ok(Some(request)) => {
                    if reader.send(Input::Op(request)).is_err() {
                        return;
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::InvalidData => {
                    eprintln!("unreadable operation: {e}")
                }
                // End of input: the service stopped or crashed, so the engine exits too.
                end => {
                    if let Err(e) = end {
                        eprintln!("cannot read operations: {e}");
                    }
                    let _ = reader.send(Input::Closed);
                    return;
                }
            }
        }
    });
    let mut engine = Engine {
        out,
        inputs,
        dir: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        session: None,
        running: None,
        model: std::env::var("BATE_MODEL").ok().filter(|m| !m.is_empty()),
        effort: None,
    };
    for input in rx {
        match input {
            Input::Op(Request { id, op }) => {
                let stop = matches!(op, Op::Stop);
                engine.handle(id, op);
                if stop {
                    break;
                }
            }
            Input::TurnDone(session) => {
                engine.running = None;
                engine.session = Some(session);
            }
            Input::Closed => break,
        }
    }
    if let Some(r) = &engine.running {
        r.control.abort();
    }
}

impl Engine {
    fn respond(&self, id: String, result: Result<(), String>) {
        self.out.send(&Event::Response {
            id,
            error: result.err(),
        });
    }

    fn idle(&self) -> Result<(), String> {
        match self.running {
            Some(_) => Err("a turn is running".into()),
            None => Ok(()),
        }
    }

    fn handle(&mut self, id: String, op: Op) {
        let saved = |e: io::Error| format!("cannot save the conversation: {e}");
        match op {
            Op::Start { .. } => match self
                .idle()
                .and_then(|()| Session::create(&self.dir).map_err(saved))
            {
                Ok(session) => {
                    self.respond(id, Ok(()));
                    self.out.send(&Event::Session {
                        session: session.id.clone(),
                    });
                    self.session = Some(session);
                }
                Err(e) => self.respond(id, Err(e)),
            },
            Op::Open { session, .. } => {
                let opened = self.idle().and_then(|()| {
                    let mut s = Session::open(&self.dir, &session)
                        .map_err(|e| format!("cannot open conversation {session}: {e}"))?;
                    let interrupted = s.close_interrupted().map_err(saved)?;
                    Ok((s, interrupted))
                });
                match opened {
                    Ok((session, interrupted)) => {
                        self.respond(id, Ok(()));
                        self.out.send(&Event::Session {
                            session: session.id.clone(),
                        });
                        for call in interrupted {
                            let output = session::INTERRUPTED.into();
                            self.out.send(&Event::ToolEnd {
                                call,
                                output,
                                is_error: true,
                            });
                        }
                        self.session = Some(session);
                    }
                    Err(e) => self.respond(id, Err(e)),
                }
            }
            Op::Prompt { nonce, text } => match self.begin_turn() {
                Ok((turn, session)) => {
                    // The response goes first: the service records the prompt before the turn's events.
                    self.respond(id, Ok(()));
                    let inputs = self.inputs.clone();
                    thread::spawn(move || {
                        let mut session = session;
                        turn.run(&mut session, nonce, text);
                        let _ = inputs.send(Input::TurnDone(session));
                    });
                }
                Err(e) => self.respond(id, Err(e)),
            },
            Op::Steer { text } => {
                let sent = match &self.running {
                    Some(r) => r.steer.send(text).map_err(|_| "the turn has ended".into()),
                    None => Err("no turn is running".into()),
                };
                self.respond(id, sent);
            }
            Op::Abort => {
                if let Some(r) = &self.running {
                    r.control.abort();
                }
                self.respond(id, Ok(()));
            }
            Op::Answer { question, choice } => {
                let taken = match &self.running {
                    Some(r) if lock(&r.control.question).as_deref() == Some(question.as_str()) => {
                        if turn::OPTIONS.contains(&choice.as_str()) {
                            r.answers
                                .send((question, choice))
                                .map_err(|_| "the turn has ended".into())
                        } else {
                            Err(format!("`{choice}` is not one of {:?}", turn::OPTIONS))
                        }
                    }
                    _ => Err(format!("question {question} is not open")),
                };
                self.respond(id, taken);
            }
            Op::SetModel { model, effort } => {
                self.model = Some(model);
                self.effort = effort;
                self.respond(id, Ok(()));
            }
            Op::Rewind { .. } | Op::Compact => {
                self.respond(id, Err("not available yet (#9)".into()))
            }
            Op::Stop => self.respond(id, Ok(())),
        }
    }

    fn begin_turn(&mut self) -> Result<(turn::Turn, Session), String> {
        self.idle()?;
        let model = self
            .model
            .clone()
            .ok_or("no model chosen: send set_model first")?;
        let provider = model::Provider::from_env()?;
        let session = self
            .session
            .take()
            .ok_or("no conversation is open: send start or open first")?;
        let control = Arc::new(Control::default());
        let (answers, answers_rx) = mpsc::channel();
        let (steer, steer_rx) = mpsc::channel();
        self.running = Some(Running {
            control: control.clone(),
            answers,
            steer,
        });
        let turn = turn::Turn {
            out: self.out.clone(),
            dir: self.dir.clone(),
            provider,
            model,
            effort: self.effort.clone(),
            control,
            answers: answers_rx,
            steer: steer_rx,
        };
        Ok((turn, session))
    }
}
