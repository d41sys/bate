//! `bate-helper`, wrapped as `Bate Helper.app` (`helper/bundle.sh`): the one program that holds
//! the macOS permissions Accessibility, Screen Recording and Full Disk Access, so the main app never needs them.
//! The service starts it and it connects to the service's helper socket; it does what a
//! `Call` says and quits when the service goes. The protocol is in `PROTOCOL.md`.

mod ax;
mod messages;

use proto::desktop::{Call, Frame, MAX_FRAME, Request};
use proto::{read_frame, write_frame};
use std::io::{self, BufReader};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::exit;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

const NO_ACCESSIBILITY: &str = "Bate Helper is not allowed to control apps. The user must allow it in System Settings > Privacy & Security > Accessibility.";
const NO_SCREEN: &str = "Bate Helper is not allowed to record the screen. The user must allow it in System Settings > Privacy & Security > Screen Recording.";

fn main() {
    // The service passes its socket when it starts the helper.
    let socket = std::env::args_os()
        .nth(1)
        .map_or_else(proto::paths::helper_socket, PathBuf::from);
    let stream = connect(&socket).unwrap_or_else(|e| {
        eprintln!("bate-helper: cannot use {}: {e}", socket.display());
        exit(1)
    });
    let out = Arc::new(Mutex::new(stream.try_clone().unwrap_or_else(|e| {
        eprintln!("bate-helper: {e}");
        exit(1)
    })));
    let status = || {
        (
            ax::trusted(),
            ax::can_capture(),
            messages::full_disk_access(),
        )
    };
    let mut last = status();
    ax::ask_for_permissions();
    send(&out, &frame(last));
    // The user grants a permission in System Settings while the helper runs.
    let watcher = out.clone();
    thread::spawn(move || {
        loop {
            thread::sleep(Duration::from_secs(2));
            let now = status();
            if now != last {
                last = now;
                send(&watcher, &frame(now));
            }
        }
    });

    let mut input = BufReader::new(stream);
    let mut buf = Vec::new();
    let mut tree: Option<ax::Tree> = None;
    loop {
        let request = match read_frame::<Request>(&mut input, MAX_FRAME, &mut buf) {
            Ok(Some(request)) => request,
            Ok(None) => exit(0),
            Err(e) if e.kind() == io::ErrorKind::InvalidData => continue,
            Err(_) => exit(0),
        };
        let reply = match run(request.call, &mut tree) {
            Ok((output, png)) => Frame::Reply {
                id: request.id,
                output: Some(output),
                png: png.map(|png| base64(&png)),
                error: None,
            },
            Err(error) => Frame::Reply {
                id: request.id,
                output: None,
                png: None,
                error: Some(error),
            },
        };
        send(&out, &reply);
    }
}

/// Connects to the service. The helper holds the user's privacy permissions, so it does what a
/// socket says only when the program on the other end is the service's own `appd`, and, when
/// the helper is signed with a certificate, signed with the same one: a program of the user's
/// that runs the helper against a socket of its own gets nothing.
// shortcut: an unsigned or ad-hoc helper (a dev run) checks the program's name only.
fn connect(socket: &Path) -> io::Result<UnixStream> {
    let stream = UnixStream::connect(socket)?;
    match proto::desktop::peer_exe(&stream) {
        Some(exe)
            if proto::desktop::is_service(&exe) && proto::signer::peer_is_same_signer(&stream) =>
        {
            Ok(stream)
        }
        _ => Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "the other end is not Bate's service",
        )),
    }
}

fn frame((accessibility, screen_recording, full_disk_access): (bool, bool, bool)) -> Frame {
    Frame::Status {
        accessibility,
        screen_recording,
        full_disk_access,
    }
}

/// A closed socket means the service is gone: so is the helper's reason to run.
fn send(out: &Mutex<UnixStream>, frame: &Frame) {
    let mut out = out.lock().unwrap_or_else(|e| e.into_inner());
    if write_frame(&mut *out, frame, MAX_FRAME).is_err() {
        exit(0);
    }
}

/// Does one call: the text for the bot, and for a screenshot the PNG.
fn run(call: Call, tree: &mut Option<ax::Tree>) -> Result<(String, Option<Vec<u8>>), String> {
    let call = match call {
        Call::Messages { query, with, limit } => {
            return messages::read(
                &messages::chat_db(),
                query.as_deref(),
                with.as_deref(),
                limit,
            )
            .map(|text| (text, None));
        }
        call => call,
    };
    let wins = ax::windows();
    let needs_accessibility = !matches!(call, Call::Apps | Call::Screenshot { .. });
    if needs_accessibility && !ax::trusted() {
        return Err(NO_ACCESSIBILITY.into());
    }
    // The numbers of the last `look`, if it was of this window.
    let known = |tree: &Option<ax::Tree>, win: &ax::Win| match tree {
        Some(t) if t.pid == win.pid && t.window == win.id => Ok(()),
        _ => Err(format!(
            "Look at {} first: element numbers come from the latest `look` of the window.",
            win.owner
        )),
    };
    match call {
        Call::Messages { .. } => unreachable!("answered above"),
        Call::Apps => Ok((ax::apps_text(&wins), None)),
        Call::Look { app, window } => {
            let (text, seen) = ax::look(ax::find(&wins, &app, window)?)?;
            *tree = Some(seen);
            Ok((text, None))
        }
        Call::Screenshot { app, window } => {
            if !ax::can_capture() {
                return Err(NO_SCREEN.into());
            }
            let win = ax::find(&wins, &app, window)?;
            let png = ax::screenshot(win)?;
            Ok((
                format!("A picture of window {} of {}.", win.id, win.owner),
                Some(png),
            ))
        }
        Call::Press {
            app,
            window,
            element,
        } => {
            let win = ax::find(&wins, &app, window)?;
            known(tree, win)?;
            let tree = tree.as_ref().expect("checked above");
            ax::press(tree, element).map(|text| (text, None))
        }
        Call::SetValue {
            app,
            window,
            element,
            value,
        } => {
            let win = ax::find(&wins, &app, window)?;
            known(tree, win)?;
            let tree = tree.as_ref().expect("checked above");
            ax::set_value(tree, element, &value).map(|text| (text, None))
        }
        Call::Type {
            app,
            window,
            text,
            element,
        } => {
            let win = ax::find(&wins, &app, window)?;
            let focus = match element {
                Some(n) => {
                    known(tree, win)?;
                    Some((tree.as_ref().expect("checked above"), n))
                }
                None => None,
            };
            ax::type_text(win.pid, focus, &text).map(|text| (text, None))
        }
    }
}

/// Standard base64, padded.
fn base64(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(DIGITS[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;

    #[test]
    fn base64_is_standard_and_padded() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(&[0xff, 0xef]), "/+8=");
    }

    #[test]
    fn the_helper_connects_only_to_the_service_program() {
        let socket = std::env::temp_dir().join(format!("helper-peer-{}.sock", std::process::id()));
        let _ = std::fs::remove_file(&socket);
        // This test program is not `appd`: a socket of its own gets no helper.
        let _listener = UnixListener::bind(&socket).unwrap();
        let refused = connect(&socket).expect_err("a stranger was trusted");
        assert_eq!(refused.kind(), io::ErrorKind::PermissionDenied);
        let _ = std::fs::remove_file(&socket);
    }
}
