//! `appd`: the per-user background service. It owns every bot, its engine,
//! the prompt ledger, open questions, routines and connectors, so work goes on
//! while no window is open. The window protocol is in `PROTOCOL.md`.

mod apple;
mod attach;
mod browser;
mod channels;
mod chatgpt;
mod clients;
mod core;
mod gate;
mod hooks;
mod lan;
mod launch;
mod limit;
mod log;
mod memory;
mod oauth;
mod paths;
mod proxy;
mod report;
mod schedule;
mod skills;
mod store;
mod sys;
mod update;

use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;
use std::process::ExitCode;
use std::sync::mpsc;
use std::thread;

use sys::log;

const USAGE: &str = "usage:
  appd              serve (launchd runs this)
  appd launch       register this appd with launchd and start it
  appd unregister   stop the service and remove its launchd registration
  appd gate chatgpt [model]
                    sign in to ChatGPT in the browser, send one request, print the reply
  appd update keygen
                    print a new key for signing releases (a private and a public half)
  appd update sign <archive> <version> [url]
                    print the signed feed.json for a release archive; the private key is in $BATE_UPDATE_KEY";

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let (arg, what) = (args.next(), args.next());
    let result = match (arg.as_deref(), what.as_deref()) {
        (None, _) => serve(),
        (Some("gate"), Some("chatgpt")) => gate::chatgpt(args.next()),
        (Some("update"), Some("keygen")) => update::keygen(),
        (Some("update"), Some("sign")) => update::sign(args.next(), args.next(), args.next()),
        (Some("launch"), _) => std::env::current_exe().and_then(|exe| launch::ensure(&exe)),
        (Some("unregister"), _) => launch::remove().map(|removed| {
            if !removed {
                println!("not registered");
            }
        }),
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            log!("appd: {e}");
            ExitCode::FAILURE
        }
    }
}

fn serve() -> io::Result<()> {
    let home = paths::home();
    for dir in [home.clone(), paths::run_dir(), paths::logs()] {
        sys::create_private_dir(&dir)?;
    }
    let Some(_lock) = sys::try_lock(&paths::lock())? else {
        // Exit 0: launchd must not restart a second copy.
        log!("another appd is already serving {}", home.display());
        return Ok(());
    };
    // Only the instance that holds the lock owns (and rotates) the log file.
    log::init(&paths::logs())?;
    skills::sweep();
    let store = store::Store::open(&paths::db()).map_err(io::Error::other)?;
    let socket = paths::socket();
    // The lock is ours, so a socket file left here is stale.
    let _ = fs::remove_file(&socket);
    let listener = UnixListener::bind(&socket)?;
    fs::set_permissions(&socket, fs::Permissions::from_mode(0o600))?;
    let (tx, rx) = mpsc::channel();
    let accept_tx = tx.clone();
    thread::spawn(move || clients::accept_loop(listener, accept_tx));
    log!("serving {} (pid {})", home.display(), std::process::id());
    let hook_port = hooks::start(tx.clone())
        .inspect_err(|e| log!("webhooks are off: {e}"))
        .ok();
    let lan = lan::Lan::start();
    core::Core::new(store, tx, hook_port, lan).run(rx);
    Ok(())
}
