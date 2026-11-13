//! `appd`: the per-user background service. It owns every bot, its engine,
//! the prompt ledger, open questions, routines and connectors, so work goes on
//! while no window is open. The window protocol is in `PROTOCOL.md`.

mod clients;
mod core;
mod launch;
mod log;
mod paths;
mod report;
mod schedule;
mod store;
mod sys;

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
  appd unregister   stop the service and remove its launchd registration";

fn main() -> ExitCode {
    let arg = std::env::args().nth(1);
    let result = match arg.as_deref() {
        None => serve(),
        Some("launch") => std::env::current_exe().and_then(|exe| launch::ensure(&exe)),
        Some("unregister") => launch::remove().map(|removed| {
            if !removed {
                println!("not registered");
            }
        }),
        Some(_) => {
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
    core::Core::new(store, tx).run(rx);
    Ok(())
}
