//! `botfile://localhost/<bot>/<path>`: a file from a bot's folder, for a sandboxed frame. The
//! page shows what a bot made (HTML, diagrams, pictures) without letting it reach the service:
//! each answer carries a policy that makes it an origin of its own, with no network.

use std::fs;
use std::path::{Path, PathBuf};
use tauri::Url;
use tauri::http::{Response, StatusCode, header};

/// `sandbox` without `allow-same-origin`: an opaque origin even if loaded on its own. No
/// `connect-src` and no remote source: nothing leaves the Mac.
pub const POLICY: &str = "sandbox allow-scripts; default-src 'none'; \
    script-src 'unsafe-inline' 'unsafe-eval' botfile:; style-src 'unsafe-inline' botfile:; \
    img-src botfile: data: blob:; font-src botfile: data:; media-src botfile: blob:; \
    form-action 'none'; base-uri 'none'";

// shortcut: a file is read whole; stream it if bots make files larger than this to show.
const MAX_BYTES: u64 = 50 << 20;

/// The answer to a request for `url`, from the bots under `home`.
pub fn serve(home: &Path, url: &str) -> Response<Vec<u8>> {
    let refuse = |status: StatusCode| {
        Response::builder()
            .status(status)
            .header(header::CONTENT_SECURITY_POLICY, POLICY)
            .body(Vec::new())
            .expect("a plain answer")
    };
    let Some(file) = locate(home, url) else {
        return refuse(StatusCode::NOT_FOUND);
    };
    match fs::metadata(&file) {
        Ok(m) if m.len() > MAX_BYTES => return refuse(StatusCode::PAYLOAD_TOO_LARGE),
        Ok(m) if m.is_file() => {}
        _ => return refuse(StatusCode::NOT_FOUND),
    }
    match fs::read(&file) {
        Ok(body) => Response::builder()
            .header(header::CONTENT_TYPE, kind(&file))
            .header(header::CONTENT_SECURITY_POLICY, POLICY)
            .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
            .body(body)
            .expect("a plain answer"),
        Err(_) => refuse(StatusCode::NOT_FOUND),
    }
}

/// The file `url` names, if it is inside its bot's folder once links are followed.
fn locate(home: &Path, url: &str) -> Option<PathBuf> {
    let url = Url::parse(url).ok()?;
    if url.scheme() != "botfile" || url.host_str() != Some("localhost") {
        return None;
    }
    // A `file:` address decodes the path; parsing already took out `.` and `..`.
    let path = Url::parse(&format!("file://{}", url.path()))
        .ok()?
        .to_file_path()
        .ok()?;
    let mut parts = path.strip_prefix("/").ok()?.components();
    let bot = parts.next()?.as_os_str().to_str()?;
    if bot.is_empty() || !bot.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return None;
    }
    let dir = home.join("bots").join(bot).canonicalize().ok()?;
    let file = dir.join(parts.as_path()).canonicalize().ok()?;
    file.starts_with(&dir).then_some(file)
}

fn kind(file: &Path) -> &'static str {
    let ext = file.extension().and_then(|e| e.to_str()).unwrap_or("");
    match ext.to_ascii_lowercase().as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "json" => "application/json",
        "pdf" => "application/pdf",
        "txt" | "md" | "csv" | "log" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_bots_own_files_are_served_and_each_with_the_sandbox() {
        let home = std::env::temp_dir().join(format!("bate-files-{}", std::process::id()));
        let bot = home.join("bots").join("b1");
        fs::create_dir_all(bot.join("out")).unwrap();
        fs::create_dir_all(home.join("bots").join("b2")).unwrap();
        fs::write(bot.join("out").join("chart one.html"), "<p>hi</p>").unwrap();
        fs::write(home.join("bots").join("b2").join("secret.txt"), "no").unwrap();
        fs::write(home.join("app.db"), "no").unwrap();
        std::os::unix::fs::symlink(home.join("app.db"), bot.join("db.html")).unwrap();

        let ok = serve(&home, "botfile://localhost/b1/out/chart%20one.html");
        assert_eq!(ok.status(), StatusCode::OK);
        assert_eq!(ok.body(), b"<p>hi</p>");
        assert_eq!(
            ok.headers()[header::CONTENT_TYPE],
            "text/html; charset=utf-8"
        );
        let policy = ok.headers()[header::CONTENT_SECURITY_POLICY]
            .to_str()
            .unwrap();
        assert!(policy.starts_with("sandbox allow-scripts;"), "{policy}");
        assert!(!policy.contains("allow-same-origin") && !policy.contains("connect-src"));

        for refused in [
            "botfile://localhost/b1/..%2fb2%2fsecret.txt",
            "botfile://localhost/b1/db.html",
            "botfile://localhost/../app.db",
            "botfile://localhost/b1/out",
            "botfile://elsewhere/b1/out/chart%20one.html",
            "tauri://localhost/b1/out/chart%20one.html",
        ] {
            let answer = serve(&home, refused);
            assert_eq!(answer.status(), StatusCode::NOT_FOUND, "{refused}");
            assert!(answer.body().is_empty(), "{refused}");
        }
        fs::remove_dir_all(&home).unwrap();
    }
}
