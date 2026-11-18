Bate is behind the other apps on 43 of 77 components, level on 17, ahead on 6, and different on 11 (8 with a stated reason).

# Bate against other apps, component by component

Compared 2026-11-18. Bate was built from its own issue list (#1 to #39) without first checking how comparable apps solve the same problems. This folder is that check, done afterwards. Each file covers one group of components, cites `path:line` for every claim, and ends with its own gap list and its own list of what was not verified.

## Reference apps

Read from shallow clones of the default branch. When apps disagree, the order of preference is pi, then omp, then the bot apps.

| App | Repo | Commit | Kind |
|---|---|---|---|
| pi | `earendil-works/pi` | `cead615b` | terminal coding agent |
| omp (oh-my-pi) | `can1357/oh-my-pi` | `7a53f0a2` | fork of pi with more features |
| OpenClaw | `openclaw/openclaw` | `fea4a9d2` | personal bot app |
| Hermes Agent | `NousResearch/hermes-agent` | `3f5a11f6` | personal bot app |
| OpenBot | `CopilotKit/OpenBot` | `6469c412` | Tauri desktop bot app |
| openbot | `nightly-labs/openbot` | `25cc00c1` | desktop app for AI teammates |
| nanobot | `HKUDS/nanobot` | `53a55658` | small personal bot app |

## Score per group

| Group | File | Rows | Bate behind | Same | Bate ahead | Different on purpose | Different, no reason found |
|---|---|---|---|---|---|---|---|
| Models and sign-in | [01](01-models-and-sign-in.md) | 10 | 6 | 2 | 0 | 2 | 0 |
| Engine | [02](02-engine.md) | 11 | 8 | 3 | 0 | 0 | 0 |
| Connectors, skills, memory | [03](03-connectors-skills-memory.md) | 7 | 4 | 1 | 0 | 0 | 2 |
| Service and storage | [04](04-service-and-storage.md) | 10 | 3 | 4 | 1 | 1 | 1 |
| Permissions and sandbox | [05](05-permissions-and-sandbox.md) | 9 | 6 | 0 | 3 | 0 | 0 |
| Channels, routines, teams | [06](06-channels-routines-teams.md) | 9 | 7 | 1 | 0 | 1 | 0 |
| Browser, Mac control, voice | [07](07-browser-computer-voice.md) | 9 | 5 | 0 | 1 | 3 | 0 |
| Window, remote access, updates | [08](08-window-remote-updates.md) | 12 | 4 | 6 | 1 | 1 | 0 |
| **Total** | | **77** | **43** | **17** | **6** | **8** | **3** |

The ChatGPT sign-in itself is in [`../chatgpt-sign-in-in-other-apps.md`](../chatgpt-sign-in-in-other-apps.md).

## The gaps that matter most

Merged from the eight files and ranked by what a person using Bate would notice. "Checked" means the Bate side was re-read in the code after the group files were written; the other apps' side is as cited in the group file.

### A. Built in the service, missing in the window

1. **Twelve service commands have no button.** The window never names `list_memory`, `read_memory`, `delete_memory`, `list_skills`, `install_skill`, `delete_skill`, `list_groups`, `set_group`, `delete_group`, `list_routine_runs`, `compact`, `steer`. So memory (#22), skills (#23), groups (#33), run history (#31), manual compaction (#9) and sending while a bot works exist only on the socket. Those issues are closed. Checked: every command name in `appd/src/core.rs` was searched for in `window/ui/src` and `window/src`. Details: 03 gap 1, 02 gap 7.

### B. A turn fails or goes wrong where other apps carry on

2. **One 429, 503 or dropped stream ends the turn.** No retry anywhere in `agentcore/src`. Checked. Copy pi's retry rules, omp's backoff. 01 gap 1.
3. **The bot is never told today's date.** Checked: no date in `agentcore/src/turn.rs` or `main.rs`. Copy omp. 02 gap 2.
4. **Every model is treated as having a 32,000-token window.** `agentcore/src/main.rs:427`. Checked. A large-window model is summarised far too early. Copy pi. 02 gap 3.
5. **No limit on steps or repeated calls**, and `run` keeps the head of long output, so the error at the end of a build log is lost. 02 gaps 4 and 6.
6. **No fallback and no provider per bot.** A ChatGPT sign-in takes over every bot; a plan limit stops them all even with a model server set up. 01 gaps 2 and 3.

### C. What the bot can see and do

7. **The bot cannot see images.** No image input in `agentcore/src`. Checked. Screenshots, photos and image files reach the model as a path. Copy pi. 02 gap 1, 07 gap 1.
8. **Browser and Mac control are thin.** Browser: open, look, click, type, back only (`appd/src/browser.rs:86`). Checked. No scroll, key press, wait or tabs; no key chords or menus in other apps. Copy omp. 07 gaps 2 and 4.
9. **No web search or page fetch tool.** 02 gap 5.
10. **No notification when a bot needs you.** Checked: no notification code in `window/` or `appd/`. Copy CopilotKit's OpenBot (same Tauri base). 07 gap 3.

### D. Safety

11. **Text from web pages, files, MCP servers and other bots reaches the model unmarked.** Bate fences only webhook data. Copy OpenClaw. 05 gap 1.
12. **The browser may open `localhost`, LAN and metadata addresses.** `appd/src/browser.rs:92-97` checks the scheme only. Checked. Copy Hermes. 05 gap 3.
13. **The service runs an Apple, browser or desktop action without checking that an approval went before it.** `appd/src/core.rs:2676-2687`. Checked: the question is decided by the service, but the action that follows is not tied to it. This relies on the engine being correct; no way for a model to exploit it was shown. Copy CopilotKit's per-operation check. 05 gap 2.
14. **A whole-disk bot reads `~/.ssh` and browser profiles without asking; no floor under `run`.** 05 gaps 4 and 5.
15. **Secrets can reach `appd.log`; connector keys sit in `app.db`.** 04 gaps 2 and 3.

### E. Chat channels and routines

16. **A bot's action cannot be approved from Telegram or Discord.** 06 gap 1.
17. **A routine's answer cannot be sent to a chat, and the bot cannot message first.** 06 gap 2.
18. **Text only, one-to-one only.** `appd/src/channels.rs:220-222` says so. Checked. No photos, voice notes, files, groups or typing indicator. 06 gaps 3, 6, 7.
19. **The bot cannot set its own reminder; no one-shot schedule.** 06 gap 4.

### F. Setup and upkeep

20. **First run has no default bot and no sign-in on the first screen.** `window/ui/src/FirstRun.tsx:27-30`. Checked. App-wide settings sit inside one bot's Settings tab. 08 gaps 1 and 2.
21. **No schema version or migration in `app.db`.** Checked: none in `appd/src/store.rs`. An update that changes a table cannot reach installed copies. 04 gap 1.
22. **The connector form cannot set headers, env or a client id; a tool with a long or dotted name is silently dropped.** 03 gaps 2 and 3.
23. **Thinking is always open; code blocks have no copy button.** 08 gap 3.

## Where Bate is ahead

- **Folder limit enforced by macOS**, not by path checks in the engine (05).
- **Bot-made HTML in sandboxed frames** with a check script (05).
- **Secrets in the Keychain** rather than plain files (04), and OAuth tokens for connectors there too (03).
- **Apple apps**: Messages, Mail, Notes, Reminders, Calendar, Contacts through a separate helper that holds the Mac permissions (07).
- **Web client on the local network** with pairing (08).

## Doc errors found on the way

- `PROTOCOL.md:302` says the chat-message trigger is "Not built yet"; `appd/src/core.rs:2156-2163` implements it.
- Issue #26's comment says channel tokens are in `app.db`; `PROTOCOL.md` says Keychain. Not checked which is right.

## Limits of this check

- Nothing was run: no app, build or test, on either side. Every "behind" is from reading code and docs.
- CopilotKit's OpenBot keeps its server in a container image that is not in the clone, so many of its cells say "not verified".
- pi and omp are terminal coding agents. They have no channels, schedules or Apple-app access, so groups 06 and 07 lean on the bot apps.
- Change sizes in the group files are estimates from reading, not from a trial.
- The Bate side of the items marked "Checked" above was re-read after the fact. The rest of the Bate side, and all of the other apps' side, was read once by the agent that wrote each group file; each file lists its own gaps under "Not verified".
