//! The window: a small shell around the macOS system web view. The page is the interface;
//! the shell gives it two calls (send a line to the service, receive the next line) and keeps
//! it on its own local files.

mod bridge;

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

/// What the web view does with an address it is asked to go to.
#[derive(Debug, PartialEq)]
enum Go {
    /// One of the page's own files.
    Stay,
    /// A link: it opens in the user's browser.
    Open,
    Refuse,
}

fn decide(url: &Url) -> Go {
    match (url.scheme(), url.host_str()) {
        ("tauri", Some("localhost")) => Go::Stay,
        ("http" | "https", Some(_)) | ("mailto", _) => Go::Open,
        _ => Go::Refuse,
    }
}

/// Whether the view goes to `url`; a link is opened in the user's browser instead.
fn follow(url: &Url) -> bool {
    match decide(url) {
        Go::Stay => true,
        Go::Open => {
            let _ = std::process::Command::new("/usr/bin/open")
                .arg(url.as_str())
                .spawn();
            false
        }
        Go::Refuse => false,
    }
}

fn main() {
    tauri::Builder::default()
        .manage(Arc::new(Bridge::start(proto::paths::socket())))
        .invoke_handler(tauri::generate_handler![send, recv])
        .setup(|app| {
            WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                .title("Bate")
                .inner_size(1000.0, 700.0)
                .on_navigation(follow)
                // `target="_blank"` and `window.open`: no second view, ever.
                .on_new_window(|url, _| {
                    follow(&url);
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
    fn only_the_pages_own_files_load_and_links_go_to_the_browser() {
        let go = |url: &str| decide(&Url::parse(url).unwrap());
        assert_eq!(go("tauri://localhost/index.html"), Go::Stay);
        assert_eq!(go("https://example.com/a"), Go::Open);
        assert_eq!(go("http://example.com/"), Go::Open);
        assert_eq!(go("mailto:ada@example.com"), Go::Open);
        for refused in [
            "tauri://elsewhere/x",
            "file:///etc/hosts",
            "javascript:alert(1)",
            "data:text/html,x",
            "ftp://example.com/",
            "about:blank",
        ] {
            assert_eq!(go(refused), Go::Refuse, "{refused}");
        }
    }
}
