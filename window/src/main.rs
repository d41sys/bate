//! The window: a small shell around the macOS system web view. The page is the interface;
//! the shell gives each window two calls (send a line to the service, receive the next line)
//! and one more (open another window), keeps it on its own local files, and adds the menus and
//! the menu-bar icon.

mod bridge;
mod files;

use bridge::Bridge;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tauri::menu::{Menu, MenuBuilder, MenuItemBuilder, SubmenuBuilder};
use tauri::tray::TrayIconBuilder;
use tauri::webview::NewWindowResponse;
use tauri::{
    AppHandle, Manager, RunEvent, State, Url, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
    WindowEvent,
};

/// Each window's own connection to the service: its page reads every line it is sent.
#[derive(Default)]
struct Bridges(Mutex<HashMap<String, Arc<Bridge>>>);

impl Bridges {
    fn of(&self, window: &str) -> Arc<Bridge> {
        let mut all = self.0.lock().unwrap_or_else(|e| e.into_inner());
        let bridge = all
            .entry(window.to_owned())
            .or_insert_with(|| Arc::new(Bridge::start(proto::paths::socket())));
        bridge.clone()
    }

    fn close(&self, window: &str) {
        let gone = self
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(window);
        if let Some(bridge) = gone {
            bridge.stop();
        }
    }
}

#[tauri::command]
fn send(bridges: State<'_, Bridges>, window: WebviewWindow, line: String) -> Result<(), String> {
    bridges.of(window.label()).send(&line)
}

#[tauri::command]
async fn recv(bridges: State<'_, Bridges>, window: WebviewWindow) -> Result<String, String> {
    let bridge = bridges.of(window.label());
    tauri::async_runtime::spawn_blocking(move || bridge.recv())
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
fn new_window(app: AppHandle) -> Result<(), String> {
    open_window(&app).map_err(|e| e.to_string())
}

/// Opens one more window on the page.
fn open_window(app: &AppHandle) -> tauri::Result<()> {
    static OPENED: AtomicUsize = AtomicUsize::new(0);
    let label = format!("bate-{}", OPENED.fetch_add(1, Ordering::Relaxed));
    let window = WebviewWindowBuilder::new(app, label, WebviewUrl::App("index.html".into()))
        .title("Bate")
        .inner_size(1000.0, 700.0)
        // Files dropped on the page reach the page (attachments), not the shell.
        .disable_drag_drop_handler()
        .on_navigation(stays)
        // A view of its own is opened by the shell alone.
        .on_new_window(|url, _| {
            open_outside(&url);
            NewWindowResponse::Deny
        })
        .build()?;
    window.set_focus()
}

/// What the menus do that the page does: the page is told the item's id.
const PAGE_ITEMS: [&str; 3] = ["new-bot", "find", "palette"];

fn menu(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    let item = |id: &str, text: &str, keys: &str| {
        MenuItemBuilder::with_id(id, text)
            .accelerator(keys)
            .build(app)
    };
    let bate = SubmenuBuilder::new(app, "Bate")
        .about(None)
        .separator()
        .services()
        .separator()
        .hide()
        .hide_others()
        .show_all()
        .separator()
        .item(&item("quit", "Quit Bate", "CmdOrCtrl+Q")?)
        .build()?;
    let file = SubmenuBuilder::new(app, "File")
        .item(&item("new-bot", "New Bot", "CmdOrCtrl+N")?)
        .item(&item("new-window", "New Window", "CmdOrCtrl+Shift+N")?)
        .separator()
        .close_window()
        .build()?;
    let edit = SubmenuBuilder::new(app, "Edit")
        .undo()
        .redo()
        .separator()
        .cut()
        .copy()
        .paste()
        .select_all()
        .separator()
        .item(&item("find", "Find…", "CmdOrCtrl+F")?)
        .build()?;
    let view = SubmenuBuilder::new(app, "View")
        .item(&item("palette", "Command Palette…", "CmdOrCtrl+K")?)
        .separator()
        .fullscreen()
        .build()?;
    let window = SubmenuBuilder::new(app, "Window")
        .minimize()
        .maximize()
        .build()?;
    MenuBuilder::new(app)
        .items(&[&bate, &file, &edit, &view, &window])
        .build()
}

/// A menu or menu-bar item was chosen.
fn chosen(app: &AppHandle, id: &str) {
    match id {
        "quit" => app.exit(0),
        "new-window" => {
            let _ = open_window(app);
        }
        "open" => match app.webview_windows().into_values().next() {
            Some(window) => {
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
            None => {
                let _ = open_window(app);
            }
        },
        id if PAGE_ITEMS.contains(&id) => {
            let windows = app.webview_windows();
            let focused = windows.values().find(|w| w.is_focused().unwrap_or(false));
            if let Some(window) = focused {
                // `id` is one of `PAGE_ITEMS`: nothing from outside goes into the script.
                let _ = window.eval(format!(
                    "dispatchEvent(new CustomEvent('bate-menu', {{ detail: '{id}' }}))"
                ));
            }
        }
        _ => {}
    }
}

/// Whether a view (the page or one of its frames) may go to `url`: only to the page's own
/// files and to bots' files. The page's `frame-src botfile:` already stops a frame going
/// elsewhere; this is the second lock, as it also runs for frames.
fn stays(url: &Url) -> bool {
    matches!(
        (url.scheme(), url.host_str()),
        ("tauri" | "botfile", Some("localhost"))
    )
}

/// The page opens a link in a new window (`target="_blank"`, `window.open`); it goes to the
/// user's browser. A bot's frame can't: its sandbox has no `allow-popups`.
fn open_outside(url: &Url) {
    let web = matches!(url.scheme(), "http" | "https") && url.host_str().is_some();
    if web || url.scheme() == "mailto" {
        // `BATE_OPENER`: `window/check-frames.sh` records what would open instead.
        let opener = std::env::var_os("BATE_OPENER").unwrap_or_else(|| "/usr/bin/open".into());
        let _ = std::process::Command::new(opener).arg(url.as_str()).spawn();
    }
}

fn main() {
    let app = tauri::Builder::default()
        .manage(Bridges::default())
        .invoke_handler(tauri::generate_handler![send, recv, new_window])
        .register_uri_scheme_protocol("botfile", |_, request| {
            files::serve(&proto::paths::home(), &request.uri().to_string())
        })
        .menu(menu)
        .on_menu_event(|app, event| chosen(app, event.id().as_ref()))
        .on_window_event(|window, event| {
            if let WindowEvent::Destroyed = event {
                window.state::<Bridges>().close(window.label());
            }
        })
        .setup(|app| {
            let handle = app.handle();
            let tray = MenuBuilder::new(handle)
                .text("open", "Open Bate")
                .text("new-window", "New Window")
                .separator()
                .text("quit", "Quit Bate")
                .build()?;
            let mut icon = TrayIconBuilder::with_id("bate").menu(&tray).tooltip("Bate");
            if let Some(image) = app.default_window_icon() {
                icon = icon.icon(image.clone());
            }
            icon.build(app)?;
            open_window(handle)?;
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("the window could not start");
    app.run(|app, event| match event {
        // The last window closed: Bate stays in the menu bar. Quit ends it.
        RunEvent::ExitRequested {
            code: None, api, ..
        } => api.prevent_exit(),
        // Clicked in the Dock with no window open.
        RunEvent::Reopen {
            has_visible_windows: false,
            ..
        } => {
            let _ = open_window(app);
        }
        _ => {}
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_may_hold_only_bots_files() {
        // The first lock on a bot's page: a frame navigating itself anywhere else is refused by
        // the web view (measured: with `https:` added here, its self-navigation got out).
        let conf = include_str!("../tauri.conf.json");
        let frames: Vec<&str> = conf
            .split(';')
            .filter_map(|d| d.trim().strip_prefix("frame-src "))
            .map(|d| d.split('"').next().unwrap_or(d))
            .collect();
        assert_eq!(frames, ["botfile:"]);
    }

    #[test]
    fn a_view_stays_on_the_pages_files_and_bots_files() {
        let stays = |url: &str| stays(&Url::parse(url).unwrap());
        assert!(stays("tauri://localhost/index.html"));
        assert!(stays("botfile://localhost/b1/chart.html"));
        for refused in [
            "https://example.com/?leak",
            "http://example.com/",
            "mailto:ada@example.com",
            "tauri://elsewhere/x",
            "botfile://elsewhere/b1/x",
            "file:///etc/hosts",
            "javascript:alert(1)",
            "data:text/html,x",
            "about:blank",
        ] {
            assert!(!stays(refused), "{refused}");
        }
    }
}
