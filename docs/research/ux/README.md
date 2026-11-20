# UX research: what is wrong with Bate's window, and how other bot apps do it

Bate's window makes everything a tab of one bot; seven open-source bot apps and the closed ones we know put the app first and the bot second, and this file ranks 15 changes that follow from that.

Issue: [#136](https://github.com/d41sys/bate/issues/136). It feeds [#137](https://github.com/d41sys/bate/issues/137) (object model) and [#138](https://github.com/d41sys/bate/issues/138) (design system). Terms follow [`GLOSSARY.md`](../../../GLOSSARY.md).

## What was studied and how

| File | What it holds |
|---|---|
| [`01-bate-today.md`](01-bate-today.md) | 75 numbered findings (`B01`…`B75`) on every screen of `window/ui/src/`, each with `path:line`. |
| [`02-how-others-do-it.md`](02-how-others-do-it.md) | Ten tasks. For each: Bate today, a table of who does what, what most share, where they differ, and a recommendation `R1`…`R15`. |

Sources:

1. Bate's window code, read in full (React files, `style.css`, `window/src/main.rs`, `tauri.conf.json`).
2. Seven open-source apps, read from clones in `/tmp/refs`: OpenClaw, Hermes, nanobot, pi, omp, `CopilotKit/OpenBot`, `nightly-labs/openbot`.
3. Closed desktop bot apps, as patterns only (no name, code or screenshot).
4. Apple Human Interface Guidelines and Nielsen's ten heuristics, read as text.

Nothing was run. The routing below is the naming the issue asks for; no issues were filed.

## Findings

| # | Finding | Bate | More in |
|---|---|---|---|
| 1 | Everything is a tab of a bot: nine tabs, 14 rem sidebar. App-wide things (Settings, Projects, Groups) sit inside a bot, and with no bot the window says "Choose a bot." | B01, B02, B50 | [02 §9](02-how-others-do-it.md) |
| 2 | In the files read, sign-in is an app-level screen in all seven other apps. In Bate it is one of 11 Settings sections, six of them app-wide. | B14, B63 | [02 §9](02-how-others-do-it.md) |
| 3 | First run: OpenClaw, Hermes and `nightly-labs/openbot` let a person get past sign-in or set it up later. Bate's first-run box needs three jumps before a first message. | B14 | [02 §1](02-how-others-do-it.md) |
| 4 | Five apps show working and needs-you in the bot list. Bate shows " · working", which also covers a bot that is waiting for an approval. | B07 | [02 §6](02-how-others-do-it.md) |
| 5 | Four apps show the approval card inline with the exact command; four surface an approval from another chat outside that chat. Bate shows the card only in the open chat. | B25 | [02 §7](02-how-others-do-it.md) |
| 6 | The model sits next to the composer in most apps and applies on pick. Bate has a separate form with a "Use" button, and "Thinking" is disabled until a model is chosen. | B22 | [02 §5](02-how-others-do-it.md) |
| 7 | All seven keep more than one conversation, with pin, rename and search. Bate keeps one endless chat per bot; search hits carry no date. | B26, B27 | [02 §8](02-how-others-do-it.md) |
| 8 | Three apps have a group chat with one timeline, an "@" list and a rule for a message with no "@". Bate's Room is a plain box with hand-typed `@name` and no Enter to send; nothing can write to a Group. | B47, B48, B59 | [02 §4](02-how-others-do-it.md) |
| 9 | OpenClaw, nanobot and Hermes show the risk before an add-on installs. Bate does so for workplaces only; Skills and Projects take typed folder paths. | B44, B58 | [02 §10](02-how-others-do-it.md) |
| 10 | Feedback is inconsistent: some deletes confirm and some act at once, Save gives no "Saved", errors appear in different places. The HIG warns only for irreversible actions. The "New bot" box sits at the foot of the sidebar, which the HIG advises against. | B05, B09, B64, B70 | [02 §2](02-how-others-do-it.md), [§9](02-how-others-do-it.md) |

Smaller findings: no `@media` rule, the pane floats below 776 px, the header is not sticky (`style.css:12-13`, `pane.ts:4-13`).

## Ranked changes

Ranked by how much friction each removes for the three common tasks: talk to a bot, start a project, see what is going on. Ease is not a factor. Size: S under a day, M a few days, L a week or more [INFERENCE, rough].

| Rank | Change | Problem (Bate) | Change | Why | Size | Belongs to |
|---|---|---|---|---|---|---|
| R1 | Top-level navigation | Everything is a bot tab; "Choose a bot" dead end; nine-tab strip (B01, B02, B50) | Sidebar with Projects, Bots, Add-ons, Settings. A bot opens to Chat and a few tabs. | Every other app does it; HIG: a sidebar leads to top-level collections ([02 §3](02-how-others-do-it.md), [§9](02-how-others-do-it.md)) | L | #137 (question 4 and its window-navigation follow-up) |
| R2 | First run and "New bot" | Sign-in is deep in Settings; first-run box takes three jumps; "New bot" at the sidebar's foot (B09, B14) | Sign-in on the first screen with a stored "Set up later"; a first bot ready; a "New bot" form with name, purpose, avatar and starter cards; "Sign in" in the chat's no-model state | The first minute decides whether the app is used ([02 §1](02-how-others-do-it.md), [§2](02-how-others-do-it.md)) | M | Own change |
| R3 | State and "needs you" in the bot list | " · working" covers waiting; no badge, no sound, no notification (B07) | Rank waiting > working > unread > idle in each row; Dock count badge; macOS notification when a bot needs the person or finishes in the background | Seeing what is going on without opening each bot ([02 §6](02-how-others-do-it.md)) | M | Own change |
| R4 | Approval card | Card shows the service's button words; only in the open chat (B25) | Show command, folder and what "Always" covers; Allow once / Always (asks again) / Deny; Enter and Esc, chords only approve; pending approvals visible from any bot | A risky action is the one place a wrong click costs most ([02 §7](02-how-others-do-it.md)) | M | Own change |
| R5 | Composer: model, effort, Stop | Separate model form; "Thinking" disabled; Queue and Steer unexplained (B22–B24) | Model chip with a scope tooltip, applied on pick, effort beside it; Stop on Esc; one-line Queue and Steer labels | Talking to a bot is the main task ([02 §5](02-how-others-do-it.md), [§6](02-how-others-do-it.md)) | M | Own change; uses #138 components |
| R6 | Project page | A project has no page and no "Open"; typed path; eight field groups (B50–B58) | Create from a folder picker; add bots in the dialog or "New bot in this project"; a page with Room, bots, notes and workplace | Starting a project is a common task ([02 §3](02-how-others-do-it.md)) | L | #137 first, then own change |
| R7 | Room as a real chat | Plain box, hand-typed `@name`, no Enter to send (B47, B48) | Enter sends; "@" opens a list and becomes a chip; Markdown; working mark and Stop per bot; the lead rule shown | The Room is where several bots meet ([02 §4](02-how-others-do-it.md)) | M | Own change |
| R8 | Many conversations per bot | One chat per bot; hits without date; Fork stays in the same chat (B26, B27) | New chat; list; auto-titles; pin and archive; dated hits with the bot named; Fork opens a new chat | Finding an old thing ([02 §8](02-how-others-do-it.md)) | L | A Conversation object to #137, then own change |
| R9 | App Settings window | 11 sections in one scroll inside a bot; no "Saved"; no ⌘, (B63–B73) | ⌘, opens Sign-in and models, Devices, Chat apps, Helper permissions, Updates, Bug report. Bot and project settings open from the bot and the project. Rows with counts. | One place per scope ([02 §9](02-how-others-do-it.md)) | M | Scope split #137; layout #138 |
| R10 | One Add-ons screen | Connectors, Skills and Workplaces in three places; typed paths; preview for workplaces only (B36–B46, B58) | Three tabs, each with Installed (switch, scope) and Browse; one install-preview dialog; folder pickers | Risk shown before install ([02 §10](02-how-others-do-it.md)) | L | Own change; uses #138 dialog |
| R11 | Feedback and errors | Inconsistent confirms and error places (B05, B64, B70) | One banner and toast; "Saved" state; inline errors; retry on connection loss; confirm only for irreversible actions, undo for the rest | NN/g heuristics 1 and 9; HIG Alerts | M | #138 |
| R12 | Empty states and plain wording | Dead ends such as "Choose a bot." (B02) | An empty-state component with one action; plain-word copy pass | A blank screen gives no next step | S | #138 component plus copy |
| R13 | Keyboard and palette | ⌘K and ⌘F exist; no ⌘,, no Esc to stop, no arrow keys in tabs | ⌘, ; Esc to stop; bot switching; palette entries for projects, settings and add-ons; arrow keys in tabs | Cheap, and every app with a palette has these | S | Own change |
| R14 | Groups | Groups cannot be written to; Projects now have a Room (B59–B62) | Fold into Projects, or drop | Two ways to group bots confuse | S | #137 (question 5) |
| R15 | Narrow screens, focus ring, contrast, reduced motion | No `@media`; pane floats below 776 px; header not sticky | One layout rule set down to a phone; visible focus; contrast; reduced motion | Accessibility and the remote view | M | #138 |

## Routing

| To | Changes |
|---|---|
| #137 (object model) | R1, R6 (first), R8 (the Conversation object), R9 (scope split), R14 |
| #138 (design system) | R11, R12, R15; layout parts of R9; components used by R5 and R10 |
| Own change (file each as an issue) | R2, R3, R4, R5, R7, R10, R13, and the build-out of R6 and R8 |

Suggested order: R1 and R6 wait for #137; R2 to R5 can start now.

## Not checked

- No window was run. Everything is from code, docs and strings.
- No text sent by the service to the window (errors, status) was read; wording findings cover what the window itself prints.
- No screen reader, no phone view, no keyboard-only pass.
- No closed app was opened for this round.
- The other six apps in [`../technology/README.md`](../technology/README.md) have no clone here.
- OpenClaw's native sidebar and projects; Hermes's group route code; omp's `collab-web` approvals; `CopilotKit/OpenBot`'s sign-in page; whether OpenClaw makes a first agent.
- Settings counts come from navigation lists. Sizes are rough guesses.
- Citations were checked by machine for file and line range, and about 20 by content; the rest were not read again.
