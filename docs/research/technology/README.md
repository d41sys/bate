Of the 13 other apps, 8 are TypeScript, 2 Python, 2 Rust and 1 Go; Bate is Rust, the smallest by a factor of six, the only one that runs on macOS alone, and closest in technology to Kwant and ZeroClaw.

# Technology inside the apps

Compared 2026-11-18. This folder answers one question: what is each app built from? It does not score features; [`../components/`](../components/README.md) does that. Each per-app file has 17 rows (runtime to licence) and cites `path:line` for every claim. The tables below are the short form of those files and carry no citations of their own.

## Apps and sizes

Read from shallow clones of the default branch. Order of preference when apps disagree: pi, then omp, then the bot apps. Files are source files tracked by git (`git ls-files`, counted by extension), the same way for every app.

| App | Repo | Commit | Source files | Largest groups | Details |
|---|---|---|---|---|---|
| pi | `earendil-works/pi` | `cead615b` | 4,754 | ts 3,546, tsx 129, mjs 66, swift 25, rs 17 | [01](01-pi-and-omp.md) |
| omp (oh-my-pi) | `can1357/oh-my-pi` | `7a53f0a2` | 10,737 | ts 6,885, rs 1,093, py 193, tsx 135 | [01](01-pi-and-omp.md) |
| OpenClaw | `openclaw/openclaw` | `fea4a9d2` | 65,792 | ts 52,033, swift 1,840, tsx 1,648, mjs 1,225, kt 657 | [02](02-openclaw-and-hermes.md) |
| Hermes Agent | `NousResearch/hermes-agent` | `3f5a11f6` | 26,910 | py 12,636, ts 4,037, tsx 1,587 | [02](02-openclaw-and-hermes.md) |
| nanobot | `HKUDS/nanobot` | `53a55658` | 2,450 | py 1,072, tsx 449, ts 434 | [03](03-nanobot-and-the-openbots.md) |
| OpenBot (CopilotKit) | `CopilotKit/OpenBot` | `6469c412` | 4,797 | ts 3,148, tsx 994, rs 103, py 100 | [03](03-nanobot-and-the-openbots.md) |
| openbot (nightly-labs) | `nightly-labs/openbot` | `25cc00c1` | 9,517 | ts 5,506, tsx 1,751, swift 23 | [03](03-nanobot-and-the-openbots.md) |
| ZeroClaw | `zeroclaw-labs/zeroclaw` | `bec292f6` | 2,421 | rs 1,269, ts 118, tsx 81, py 51 | [04](04-zeroclaw-picoclaw-nanoclaw.md) |
| PicoClaw | `sipeed/picoclaw` | `bbf6893c` | 1,393 | go 868, tsx 114, ts 61 | [04](04-zeroclaw-picoclaw-nanoclaw.md) |
| NanoClaw | `nanocoai/nanoclaw` | `7bf3e9c7` | 1,395 | ts 1,006 | [04](04-zeroclaw-picoclaw-nanoclaw.md) |
| Kwant | `alexsergivan/kwant` | `b4b040c5` | 2,872 | rs 651, mjs 265, tsx 223, ts 184 | [05](05-kwant-brigade-aionui.md) |
| Brigade | `spinabot/brigade` | `6aa84b6b` | 1,590 | ts 1,404 | [05](05-kwant-brigade-aionui.md) |
| AionUi | `iOfficeAI/AionUi` | `e108b6b5` | 2,347 | ts 1,052, tsx 563 | [05](05-kwant-brigade-aionui.md) |
| **Bate** | `d41sys/bate` | `78f8217` | 212 | rs 62, tsx 15, ts 15, sh 4 | [06](06-bate.md) |

Three repos do not contain their own engine, so their engine rows describe only what is visible:

- **AionUi:** the agent loop, storage, scheduling and channels are in a separate Rust binary, `aioncore`, built from another repo.
- **OpenBot (CopilotKit):** the server, the bot computer and the supervisor ship as a published container image; their source is not in the repo.
- **openbot (nightly-labs):** it downloads and drives vendor agent programs, so agent behaviour lives in those.

## Language, runtime and desktop shell

| App | Main language | Runtime | Desktop shell | UI toolkit |
|---|---|---|---|---|
| pi | TypeScript | Node 22.19 or newer; the standalone binary is compiled with Bun | none (terminal program) | own terminal renderer, `pi-tui` |
| omp | TypeScript and Rust | Bun 1.4.4 or newer; 12 Rust crates loaded in-process as one N-API addon | none | fork of `pi-tui` backed by Rust; React 19 for two side web UIs |
| OpenClaw | TypeScript, with Swift and Kotlin apps | Node 24, pnpm | native Swift/SwiftUI on macOS; Tauri 2 on Linux; native iOS and Android apps | Solid 2 web UI; `pi-tui` 1.4.0 terminal UI |
| Hermes Agent | Python, with TypeScript UIs | Python 3.11 to 3.14; Node 26 for the UIs | Electron 44 | React 19, Tailwind 4, Radix; own fork of Ink for the terminal |
| nanobot | Python | Python 3.11 or newer; Bun for the terminal UI | none in the repo | React 18 web UI; OpenTUI terminal UI |
| OpenBot (CopilotKit) | TypeScript | Bun 1.4.3; the backend runs as Docker or Podman containers | Tauri 2, used only to install and start the containers | React 19, Tailwind 4, TanStack, CopilotKit |
| openbot (nightly-labs) | TypeScript | Bun 1.4.2, Node 24 | Electron 45 | SolidJS 2, Kobalte, Tailwind 4 |
| ZeroClaw | Rust | Rust 1.96 or newer, tokio | Tauri 2 tray app that loads the daemon's web dashboard | React 19 dashboard; ratatui terminal UI |
| PicoClaw | Go | Go 1.25, built without CGO | none; a Go web server with a tray icon (`fyne.io/systray`) | React 19, shadcn and Radix, Tailwind 4 |
| NanoClaw | TypeScript | Node 22 on the host; Bun 1.4.4 inside the container | none | terminal prompts only |
| Kwant | Rust | Rust 2021, Tokio and Axum | Tauri 2 | React 18, Tailwind 3 |
| Brigade | TypeScript | Node 22.12 or newer | none | pi's `pi-tui` 0.79.9 |
| AionUi | TypeScript | Node 22 to 24; Bun for installs | Electron 37 | React 19, Arco Design, UnoCSS |
| **Bate** | Rust | Rust 1.92, edition 2024; threads, no async runtime in the service or engine | Tauri 2 | React 19, no component library or CSS framework |

## Processes and storage

| App | Processes and how they talk | Database | Conversations and memory |
|---|---|---|---|
| pi | one Node process; JSON lines on stdin/stdout in RPC mode | none in the terminal program | JSONL files |
| omp | one Bun process with Rust in-process; child workers for speech | `bun:sqlite` | JSONL files; `mnemopi` memory in SQLite with optional embeddings |
| OpenClaw | one gateway; clients and devices connect over WebSocket with JSON frames | Node's built-in `node:sqlite` with Kysely; Postgres optional | SQLite; optional vectors (`sqlite-vec`, pgvector, LanceDB) |
| Hermes Agent | one Python gateway; every UI connects over WebSocket JSON-RPC | Python's built-in `sqlite3` with FTS5 | SQLite; `MEMORY.md` and `USER.md` |
| nanobot | one Python gateway; UIs over WebSocket and REST | Python's built-in `sqlite3` | SQLite; `MEMORY.md` and `history.jsonl` |
| OpenBot (CopilotKit) | several containers (server, bot computer, supervisor, agent) over REST, AG-UI and WebSocket | PostgreSQL 17 | threads and memory in CopilotKit Intelligence, outside the repo |
| openbot (nightly-labs) | Electron main, renderer and utility processes; one child process per agent program over stdio | `node:sqlite`, stored as events | SQLite |
| ZeroClaw | one daemon; JSON-RPC over a Unix socket; HTTP and WebSocket gateway (axum) | `rusqlite` 0.40, bundled; Postgres and Qdrant optional | SQLite with FTS5 and embeddings |
| PicoClaw | a launcher starts the gateway; WebSocket and REST | `modernc.org/sqlite` (pure Go) | JSONL files; SQLite FTS5 index; Markdown files |
| NanoClaw | one Node host and one Docker container per conversation; they exchange messages through two SQLite files each side polls | `better-sqlite3` on the host, `bun:sqlite` in the container | Claude's own `.jsonl` transcripts; Markdown memory |
| Kwant | one Tokio/Axum server inside the Tauri app; one child process per turn for agent programs; SSE and WebSocket to the UI | SQLite through `sqlx` 0.8 | SQLite; memory search over embeddings plus FTS5 |
| Brigade | one gateway; clients over WebSocket | none (files); Convex optional | pi's JSONL transcripts; `MEMORY.md` |
| AionUi | Electron main and renderer, plus the `aioncore` child over HTTP and WebSocket | inside `aioncore`, not visible | not visible |
| **Bate** | `appd` service as a login item; one `agentcore` child per bot over JSON lines on stdin/stdout; window over a Unix socket; other devices over HTTPS long-polling | `rusqlite` 0.37, bundled | JSONL files; Markdown memory; no vectors |

## Model calls and agent loop

| App | How it calls models | Whose agent loop | Other engines it runs |
|---|---|---|---|
| pi | vendor SDKs (`openai`, `@anthropic-ai/sdk`, `@google/genai`, AWS Bedrock) | own (`pi-agent-core`, 952 lines) | none |
| omp | own HTTP and own SSE reader; no vendor SDK | own, forked from pi's (4,206 lines) | none |
| OpenClaw | own package wrapping the vendor SDKs | own, adapted from pi's | Codex app-server; ACP agents |
| Hermes Agent | `openai` SDK plus own adapters; Anthropic SDK optional | own, in Python | none |
| nanobot | vendor SDKs (`anthropic`, `openai`, `boto3`) | own, in Python | none |
| OpenBot (CopilotKit) | whatever each framework uses (LangChain, LiteLLM, AI SDK and others) | none of its own in the repo: 13 agent frameworks behind the AG-UI protocol, LangGraph by default | any AG-UI endpoint |
| openbot (nightly-labs) | not directly; through vendor programs and SDKs | none of its own | Codex app-server, Claude Agent SDK, pi over RPC, Muse, ACP agents |
| ZeroClaw | own `reqwest` clients and own SSE parsing | own, in Rust | optional tools that hand work to Claude Code, Codex, Gemini and OpenCode programs |
| PicoClaw | mixed: official Go SDKs for Anthropic, OpenAI, Bedrock and Copilot; own HTTP for OpenAI-compatible servers and Gemini | own, in Go | none |
| NanoClaw | Claude Agent SDK | Claude Code's, inside a container | Codex, OpenCode, Ollama through install skills |
| Kwant | own `reqwest` calls and hand-written SSE | own, for API models | `claude -p`, `codex exec` and `grok`, one child process per turn |
| Brigade | pi's `pi-ai` 0.79.9 | pi's, in-process (`pi-coding-agent` 0.79.9) | Claude Code as a child process |
| AionUi | vendor SDKs are in the repo; use at run time not visible | none in the repo (`aioncore`) | agent programs over ACP (Claude Code, Codex, Gemini CLI and others, per its README) |
| **Bate** | own HTTP with `ureq` 3 (chat completions and Responses SSE); no vendor SDK | own, in Rust | ACP agents installed on the Mac |

## Extension protocols and sandbox

| App | MCP | ACP | What limits the commands a bot runs |
|---|---|---|---|
| pi | own client | none | nothing built in (a micro-VM is offered as an example extension) |
| omp | own client | own server | no OS sandbox; separate worktrees or overlays for subagents |
| OpenClaw | official SDK | client | Docker, Podman or SSH backends; no OS sandbox of its own |
| Hermes Agent | official SDK, client and server | server | Docker, Modal, Daytona, SSH or Vercel backends; no OS sandbox |
| nanobot | official SDK, client | none | bubblewrap on Linux, `sandbox-exec` on macOS, job objects on Windows |
| OpenBot (CopilotKit) | client inside the server (not visible) | none; AG-UI instead | one container per bot; gVisor optional |
| openbot (nightly-labs) | official SDK, client and a local server | client, official SDK | `sandbox-exec` on macOS only; refuses on Linux |
| ZeroClaw | own client | server and bridge | Landlock, Bubblewrap, Firejail, Docker or `sandbox-exec`; plugins run as WebAssembly in wasmtime |
| PicoClaw | official Go SDK, client | none | `bwrap` on Linux; restricted token and Job Object on Windows; nothing on macOS |
| NanoClaw | official SDK | none | Docker container per conversation with capabilities dropped |
| Kwant | own client, plus own gateway that serves its tools to agent programs | none | Docker with no network; `sandbox-exec` on macOS when switched on |
| Brigade | own server only; no client found | none ("permanently dropped") | nothing; pattern guards only |
| AionUi | official SDK for one built-in server; the rest in `aioncore` | the main protocol to agent programs | nothing in the repo |
| **Bate** | own client | own client | macOS `sandbox-exec` |

## Browser, computer and voice

| App | Browser control | Computer control | Voice |
|---|---|---|---|
| pi | none | none | none in the terminal program |
| omp | `puppeteer-core`; Chrome extension for existing tabs | own Rust code (AT-SPI on Linux, AppKit on macOS, UI Automation on Windows) | on-device: sherpa-onnx in, `ptts` out; live voice over WebRTC |
| OpenClaw | `playwright-core` over CDP; `chrome-devtools-mcp`; Chrome extension | `cua-driver`; Peekaboo on macOS | provider plugins; on-device Apple SpeechAnalyzer, MLX and ONNX |
| Hermes Agent | `browser-harness`; raw CDP | `cua-driver` over MCP | on-device faster-whisper or whisper.cpp in; edge-tts, cloud or local models out |
| nanobot | none | none | cloud speech-to-text only |
| OpenBot (CopilotKit) | Chromium in the bot's container (library not visible) | the same container | on-device: Apple `SFSpeechRecognizer`, Windows SAPI, the browser's recogniser; no speech out |
| openbot (nightly-labs) | Electron web view driven by raw CDP | `cua-driver`; Sunshine and Moonlight for remote desktop | on-device sherpa-onnx (Parakeet, Whisper, Bark); ElevenLabs |
| ZeroClaw | external `agent-browser` program, WebDriver (`fantoccini`), or an HTTP computer-use endpoint | `screencapture` and `osascript` in the tray app | cloud in and out; wake word |
| PicoClaw | none in Go; `agent-browser` as a skill in the larger Docker image | none | cloud in and out |
| NanoClaw | `agent-browser` and Chromium inside the container | none | none |
| Kwant | raw CDP over WebSocket | none found | cloud speech-to-text only |
| Brigade | `playwright-core` | none | cloud in and out; free Edge speech out |
| AionUi | `chrome-devtools-mcp` over Electron's debugger | not visible | speech-to-text through the backend; no speech out |
| **Bate** | Chrome over CDP on a pipe, not a network port | macOS Accessibility and window capture in a helper; AppleScript | the system's Dictation in; the web view's `speechSynthesis` out |

## Channels and scheduling

| App | Messaging channels | Built with | Scheduling |
|---|---|---|---|
| pi | Discord, in the experimental server only | `discord.js` | none |
| omp | none | n/a | none |
| OpenClaw | Telegram, Discord, Slack, WhatsApp, Matrix, Teams, email, LINE and more | one library per channel, as plugins (grammY, Bolt, Baileys and others) | `croner` |
| Hermes Agent | 26 platform plugins | python-telegram-bot, discord.py, slack-bolt, mautrix; Baileys in a Node bridge | `croniter` with own scheduler |
| nanobot | Telegram, Discord, Slack, Matrix, WhatsApp, Feishu, email and more | one optional Python library per channel | `croniter` with own service |
| OpenBot (CopilotKit) | Slack and Teams, through a separate project | AG-UI | own, on a PostgreSQL work queue |
| openbot (nightly-labs) | Telegram, Discord, Slack, WhatsApp | a relay service: direct Bot API, `@discordjs/*`, vendored Baileys | own timer; no cron library |
| ZeroClaw | Telegram, Discord, Slack, Matrix, WhatsApp, email, Nostr and more | own clients for Telegram, Discord and Slack; library crates for Matrix, Nostr, WhatsApp and email | `cron` crate with own scheduler |
| PicoClaw | Telegram, Discord, Slack, WhatsApp, Matrix, Feishu, LINE, QQ and more | one Go library per channel | `gronx` with own service |
| NanoClaw | terminal only as shipped; others are added by install skills | Vercel Chat SDK adapters; Baileys | `cron-parser` with own sweep |
| Kwant | Telegram | own Bot API client | own; no cron crate |
| Brigade | WhatsApp, Telegram, Discord, Slack, iMessage | Baileys, grammY, discord.js, Slack SDK | `croner` |
| AionUi | Telegram, Lark, DingTalk, WeCom, per its README | inside `aioncore`, not visible | inside `aioncore` |
| **Bate** | Telegram, Discord | own Bot API and gateway clients; no bot library | own, on the `jiff` time library |

## Secrets, packaging, updates and licence

| App | Secrets at rest | Ships as | Updates | Licence |
|---|---|---|---|---|
| pi | plain `auth.json`, mode 0600 | npm, compiled binaries, install script, Nix | `pi update` | MIT |
| omp | SQLite `agent.db`, mode 0600 | binaries, Homebrew, npm, Nix, Docker | `omp update` | MIT |
| OpenClaw | SQLite or plain files on the gateway; Keychain in the Apple apps; 1Password and Vault plugins | npm, Docker, signed macOS app, AppImage and deb, mobile apps | Sparkle on macOS; Tauri updater on Linux | MIT |
| Hermes Agent | `.env` and `auth.json`; encrypted vault file; OS keychain optional | install script, Docker, Electron installers, Nix | `electron-updater`; `hermes update` | MIT |
| nanobot | plain `config.json`, mode 0600 | PyPI, Docker, terminal binaries | `nanobot update` | MIT |
| OpenBot (CopilotKit) | server: encrypted with a key from the environment; desktop: OS keychain | Tauri installers, container images, Helm charts, mobile app | Tauri updater | MIT; CopilotKit Intelligence is licensed separately |
| openbot (nightly-labs) | Electron `safeStorage` (Keychain on macOS), key file as fallback | Electron installers, Docker, mobile app | `electron-updater` | PolyForm Noncommercial 1.0.0 |
| ZeroClaw | encrypted in `config.toml` (ChaCha20-Poly1305), key file mode 0600 | archives for 10 targets, Tauri bundles, Docker, crates.io, AUR, Scoop, Nix, firmware | `zeroclaw update` with rollback | MIT or Apache-2.0 |
| PicoClaw | `.security.yml`; AES-256-GCM optional | archives, deb and rpm, Docker, DMG, Windows installer, Android | self-update from GitHub releases | MIT |
| NanoClaw | a credential gateway, so the agent never holds keys; host tokens in `.env` | git clone and a setup script; a container image | an update skill | MIT |
| Kwant | AES-256-GCM vault files for site passwords; API keys in SQLite; Bitwarden optional | dmg, deb, AppImage, Windows beta, Docker, Homebrew | Tauri updater, signed | AGPL-3.0-only; a commercial licence is offered |
| Brigade | files under `~/.brigade/`, mode 0600 | npm | `brigade update` (npm) | MIT |
| AionUi | not visible; no keychain use found in the repo | Electron installers, web runtime, Docker, mobile app | `electron-updater` | Apache-2.0 |
| **Bate** | macOS Keychain only | one macOS app | own feed, Ed25519 signature check, atomic swap | Apache-2.0 |

Tests and quality tooling are row 17 of each per-app file.

## Where Bate sits

Counted over all 14 apps, from the tables above.

- **Closest relatives:** Kwant (Rust, Tauri 2, React, SQLite, own HTTP model calls, raw CDP, own Telegram client, own scheduler) and ZeroClaw (Rust, Tauri 2, bundled `rusqlite`, own HTTP model calls, own MCP client, `sandbox-exec` as one of its sandboxes). Both are built on tokio and also run on Linux and Windows; Bate has no async runtime and runs on macOS only.
- **Own code where others take a library:** Bate, omp, ZeroClaw and Kwant call model APIs without a vendor SDK. Bate, pi, omp, ZeroClaw and Kwant have their own MCP client. Of the apps with a desktop shell, Bate and ZeroClaw update with their own code; the rest use Sparkle, the Tauri updater or `electron-updater`.
- **Only in Bate:** the Keychain as the only place secrets are kept, and the system's Dictation command for speech in.
- **Not in Bate, present in several others:** a terminal UI (7: pi, omp, OpenClaw, Hermes Agent, nanobot, ZeroClaw, Brigade), vector or embedding search (4: ZeroClaw and Kwant; omp and OpenClaw as options), on-device speech models (4: omp, OpenClaw, Hermes Agent, openbot), a plugin system beyond MCP and skills, a Linux or Windows build, a mobile app (4: OpenClaw, OpenBot, openbot, AionUi).
- **Same as most:** SQLite (12 of 14 use it somewhere; OpenBot and Brigade do not), `SKILL.md` skills (11), JSONL conversation files (pi, omp, PicoClaw, Brigade, Bate), Tauri 2 as the shell (Bate, Kwant, ZeroClaw, OpenBot, and OpenClaw on Linux).

## How this was checked

1. Five readers with no shell each read two or three repos and wrote the per-app tables with `path:line` citations.
2. Commits: all 13 confirmed with `git rev-parse` against the headings.
3. Citations: a script resolved every cited path in files 01 to 05 to a file tracked in that repo with at least as many lines as the cited line number. 1,394 of 1,394 resolved, after one path that lacked its folders was corrected. A path cited without its folders was matched by its ending.
4. Content: 37 citations behind the tables above were read line by line. 36 said what the cell says; one line number was off by one and is corrected.
5. Bate's citations were read at `78f8217`, and all 63 path and line references in file 06 resolve.

## Not verified

- **Content of the other ~1,360 citations.** Step 3 proves the file and line exist, not that the line says what the cell claims.
- **"Not found" cells.** Each rests on searches of named folders, listed in the cell, not of the whole tree.
- **Engines outside the repos:** `aioncore` (AionUi), the CopilotKit server image, and the vendor programs openbot drives were not read.
- **Nothing was built or run.** Versions are the ones the manifests pin, not ones observed running.
- **The per-app notes are in the readers' own words** ("I searched…"); they were kept as written.
