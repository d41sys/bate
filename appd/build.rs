//! Builds the window's page (window/ui/dist) into the service, for other devices on the
//! network (`lan`). Without a built page the service still builds, and says so when asked.
use std::path::{Path, PathBuf};
use std::{env, fs};

fn main() {
    let dist = Path::new(env!("CARGO_MANIFEST_DIR")).join("../window/ui/dist");
    println!("cargo:rerun-if-changed={}", dist.display());
    let mut files = Vec::new();
    walk(&dist, &mut files);
    files.sort();
    let mut out = String::from("pub const FILES: &[(&str, &[u8])] = &[\n");
    for file in &files {
        let rel = file
            .strip_prefix(&dist)
            .expect("inside dist")
            .to_string_lossy()
            .replace('\\', "/");
        let abs = file.canonicalize().expect("a file of dist");
        out.push_str(&format!(
            "    ({:?}, include_bytes!({:?})),\n",
            format!("/{rel}"),
            abs
        ));
    }
    out.push_str("];\n");
    let target = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR")).join("page.rs");
    fs::write(target, out).expect("page.rs written");
}

fn walk(dir: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        println!("cargo:rerun-if-changed={}", path.display());
        if path.is_dir() {
            walk(&path, files);
        } else {
            files.push(path);
        }
    }
}
