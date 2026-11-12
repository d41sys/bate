//! Window connections: an acceptor thread, plus a reader thread and a writer
//! thread per client, so a slow client never blocks the core.

use std::io::{self, BufReader, Write};
use std::net::Shutdown;
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::Duration;

use proto::{MAX_WINDOW_FRAME, read_frame};
use serde_json::Value;

use crate::core::Msg;
use crate::sys::{self, log};

/// A client whose unsent output exceeds this is dropped.
const MAX_QUEUED_BYTES: usize = 128 << 20;

/// The core's side of one client: a queue into its writer thread.
pub struct ClientHandle {
    tx: Sender<Arc<[u8]>>,
    queued: Arc<AtomicUsize>,
    stream: UnixStream,
}

impl ClientHandle {
    /// Queues one encoded frame. `false` means the client is gone or too slow
    /// and must be dropped.
    pub fn send(&self, bytes: &Arc<[u8]>) -> bool {
        let total = self.queued.fetch_add(bytes.len(), Ordering::Relaxed) + bytes.len();
        total <= MAX_QUEUED_BYTES && self.tx.send(Arc::clone(bytes)).is_ok()
    }

    /// Closes the socket; both client threads then end.
    pub fn close(&self) {
        let _ = self.stream.shutdown(Shutdown::Both);
    }
}

pub fn accept_loop(listener: UnixListener, tx: Sender<Msg>) {
    let mut next_id = 0u64;
    for stream in listener.incoming() {
        let stream = match stream {
            Ok(s) => s,
            Err(e) => {
                log!("accept failed: {e}");
                thread::sleep(Duration::from_millis(100));
                continue;
            }
        };
        if !sys::peer_is_self(&stream) {
            log!("refused a connection from another user");
            continue;
        }
        next_id += 1;
        if let Err(e) = serve(stream, next_id, &tx) {
            log!("client {next_id}: cannot start: {e}");
        }
    }
}

fn serve(stream: UnixStream, id: u64, tx: &Sender<Msg>) -> io::Result<()> {
    let reader_stream = stream.try_clone()?;
    let writer_stream = stream.try_clone()?;
    let (out_tx, out_rx) = mpsc::channel::<Arc<[u8]>>();
    let queued = Arc::new(AtomicUsize::new(0));
    let handle = ClientHandle {
        tx: out_tx,
        queued: Arc::clone(&queued),
        stream,
    };
    // Registered before the reader exists, so the core sees the client first.
    tx.send(Msg::ClientConnected { id, handle })
        .map_err(|_| io::Error::other("core is gone"))?;
    thread::spawn(move || write_loop(writer_stream, out_rx, queued));
    let tx = tx.clone();
    thread::spawn(move || read_loop(reader_stream, id, tx));
    Ok(())
}

fn write_loop(mut stream: UnixStream, rx: mpsc::Receiver<Arc<[u8]>>, queued: Arc<AtomicUsize>) {
    for bytes in rx {
        let sent = stream.write_all(&bytes).and_then(|()| stream.flush());
        queued.fetch_sub(bytes.len(), Ordering::Relaxed);
        if sent.is_err() {
            break;
        }
    }
    let _ = stream.shutdown(Shutdown::Both);
}

fn read_loop(stream: UnixStream, id: u64, tx: Sender<Msg>) {
    let mut reader = BufReader::new(stream);
    let mut buf = Vec::new();
    loop {
        let msg = match read_frame::<Value>(&mut reader, MAX_WINDOW_FRAME, &mut buf) {
            Ok(Some(frame)) if frame.is_object() => Msg::ClientFrame { id, frame },
            Ok(Some(_)) => Msg::ClientGarbage { id },
            Ok(None) => break,
            Err(e) if e.kind() == io::ErrorKind::InvalidData => Msg::ClientGarbage { id },
            // Over the cap (the rest of the line is unread) or a broken socket.
            Err(e) => {
                log!("client {id}: closing: {e}");
                break;
            }
        };
        if tx.send(msg).is_err() {
            return;
        }
    }
    let _ = tx.send(Msg::ClientGone { id });
}
