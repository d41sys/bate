# Bate

A personal bot app for the Mac. You create bots, give each one a job, and talk to them. A bot can use your own AI subscription, read and change files, run commands, use connected services and run on a schedule. A background service owns the work, so it continues when the window is closed.

Bate runs entirely on your Mac. The project runs no server.

## Status

Early development. Nothing here is a release yet, and no code is published yet. Each part arrives in this repository as it is written.

| Part | What it is |
|---|---|
| Sign-in | Sign in to your ChatGPT subscription, or use an API key or a local model. Claude runs through its own command-line program, as an installed agent (ACP) |
| Engine | Bate's own agent loop, written in Rust |
| Background service | Owns the bots, their history, schedules and permission questions |
| Window | A Rust shell around the macOS system web view, with a React interface |

The plan is in the [issues](../../issues), grouped by release milestone.

## Requirements

macOS 13 or later on Apple Silicon. The Rust version is pinned in `rust-toolchain.toml`.

The window's page needs Node 24 or later: run `npm ci && npm run build` in `window/ui` before building the Rust workspace. `cargo run -p window` then opens the window. Files a bot links to in a reply open in a sandboxed frame (`botfile:`) with no network and no way to the service; `window/check-frames.sh` shows this in a real window.

Bate keeps its secrets (model key, sign-ins, channel tokens) in the macOS Keychain, not in `app.db`. The Keychain recognises a program by its signature, so a build signed ad hoc is a new program after every rebuild and macOS asks again. To avoid that, make a self-signed code-signing certificate named `Bate Dev` once (the script prints the steps) and run `window/sign-dev.sh` after each `cargo build`; it signs `appd`, `agentcore` and `bate` under `target/debug` and `target/release`.

## Licence

Apache-2.0. See [`LICENSE`](LICENSE) and [`NOTICE`](NOTICE).
