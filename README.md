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

Bate keeps its secrets (model key, sign-ins, channel tokens) in a macOS keychain, not in `app.db`. A build signed with a Developer ID uses the login keychain. A build from source has no Team ID, and the login keychain asks about every secret again after every rebuild. To avoid that, run `window/dev-identity.sh` once: it makes a self-signed code-signing certificate named `Bate Dev` in a keychain file of its own and asks nothing. `window/sign-dev.sh`, run after each `cargo build`, signs `appd`, `agentcore` and `bate` under `target/debug` and `target/release` with it. A build signed that way keeps its secrets in `secrets.keychain` in Bate's data folder, where macOS goes by the signer alone and asks nothing after a rebuild. That file is protected like `app.db`, by the folder being readable to you alone, not by your login password: a bot you gave the whole disk can read both. A secret an earlier version stored in the login keychain is moved at its first use, and macOS asks about it that one time (choose Allow). A build signed ad hoc stays with the login keychain and is asked about at every rebuild. The question appears while Bate goes on working: a request that needs that secret says that macOS is asking and can be sent again once you have answered. If you say no, Bate does not ask again by itself; Settings, under ChatGPT, then has an **Ask again** button.

`window/bundle.sh` builds `Bate.app` (signed with `Bate Dev` once `window/dev-identity.sh` has run, else ad hoc). Copy it to `/Applications` and open it: the app registers the background service itself, and the first screen points at Settings for ChatGPT or a model server. Notarisation needs an Apple Developer ID and is off until `BATE_NOTARIZE_PROFILE` is set; see `PROTOCOL.md`, "The app".

## Licence

Apache-2.0. See [`LICENSE`](LICENSE) and [`NOTICE`](NOTICE).
