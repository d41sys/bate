//! The window: a small shell around the macOS system web view. The page is the interface;
//! the shell gives it two calls (send a line to the service, receive the next line) and keeps
//! it on its own local files.

mod bridge;
mod files;

use bridge::Bridge;
use std::sync::Arc;
use tauri::webview::NewWindowResponse;
use tauri::{State, Url, WebviewUrl, WebviewWindowBuilder};

#[tauri::command]
fn send(bridge: State<'_, Arc<Bridge>>, line: String) -> Result<(), String> {
    bridge.send(&line)
}

#[tauri::command]
async fn recv(bridge: State<'_, Arc<Bridge>>) -> Result<String, String> {
    let bridge = bridge.inner().clone();
    tauri::async_runtime::spawn_blocking(move || bridge.recv())
        .await
        .map_err(|e| e.to_string())?
}

/// Whether a view (the page or one of its frames) may go to `url`: only to the page's own
/// files and to bots' files. This also runs for frames, so a bot's page can't send anything out
/// by going to an address of its own choosing.
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
    tauri::Builder::default()
        .manage(Arc::new(Bridge::start(proto::paths::socket())))
        .invoke_handler(tauri::generate_handler![send, recv])
        .register_uri_scheme_protocol("botfile", |_, request| {
            files::serve(&proto::paths::home(), &request.uri().to_string())
        })
        .setup(|app| {
            WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                .title("Bate")
                .inner_size(1000.0, 700.0)
                // Files dropped on the page reach the page (attachments), not the shell.
                .disable_drag_drop_handler()
                .on_navigation(stays)
                // No second view, ever.
                .on_new_window(|url, _| {
                    open_outside(&url);
                    NewWindowResponse::Deny
                })
                .build()?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("the window could not start");
}

#[cfg(test)]
mod tests {
    use super::*;

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
