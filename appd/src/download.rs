//! One streaming download, shared by app updates and workplace installs: the bytes are hashed
//! as they arrive and written to a file, and never more than `max` of them are taken.

use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use std::time::Duration;

use ring::digest::{Context, SHA256};

/// An agent that follows at most `redirects` redirects and gives up after `total`.
pub fn agent(redirects: u32, total: Duration) -> ureq::Agent {
    let config = ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(15)))
        .timeout_global(Some(total))
        .max_redirects(redirects)
        .build();
    ureq::Agent::new_with_config(config)
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// What was downloaded.
pub struct Got {
    pub sha256: String,
    pub size: u64,
}

/// Writes the body of `url` to `path`, hashing it on the way; more than `max` bytes is an
/// error. `progress` gets the bytes so far after each piece. The caller compares the hash and
/// removes `path` when it fails: a file may be left behind on an error.
pub fn to_file(
    agent: &ureq::Agent,
    url: &str,
    max: u64,
    path: &Path,
    mut progress: impl FnMut(u64),
) -> Result<Got, String> {
    let err = |what: &str, e: &dyn std::fmt::Display| format!("{what}: {e}");
    let too_big = || format!("the download is larger than the {max} bytes it may be");
    let response =
        (agent.get(url).call()).map_err(|e| err(&format!("cannot download {url}"), &e))?;
    // An agent that follows no (more) redirects hands the redirect itself back.
    if response.status().is_redirection() {
        return Err(format!(
            "cannot download {url}: it answered {}, a redirect that is not followed",
            response.status()
        ));
    }
    let mut body = response.into_body();
    if body.content_length().is_some_and(|n| n > max) {
        return Err(too_big());
    }
    let mut file = File::create(path).map_err(|e| err("cannot write the download", &e))?;
    let (mut hash, mut buf, mut size) = (Context::new(&SHA256), [0u8; 64 << 10], 0u64);
    let mut reader = body.as_reader();
    loop {
        let n = reader
            .read(&mut buf)
            .map_err(|e| err("the download broke off", &e))?;
        if n == 0 {
            break;
        }
        size += n as u64;
        if size > max {
            return Err(too_big());
        }
        hash.update(&buf[..n]);
        file.write_all(&buf[..n])
            .map_err(|e| err("cannot write the download", &e))?;
        progress(size);
    }
    Ok(Got {
        sha256: hex(hash.finish().as_ref()),
        size,
    })
}
