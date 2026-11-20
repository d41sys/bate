The window has no way in to anything except a bot, and 6 of the 11 sections of a bot's Settings page belong to the whole app.

# Bate's window today: every screen, in the order a person meets it

Read 2026-11-20 from the code in `window/ui/src/` on branch `issue-136-ux-research` (cut from `main`). This file answers one question: what does a person try to do on each screen, and what gets in the way? It does not say how to fix it; [`02-how-others-do-it.md`](02-how-others-do-it.md) and the ranked list in [`README.md`](README.md) do. Every row gives `path:line`; paths are inside `window/ui/src/` unless they say otherwise.

How to read the tables:

- **Tries** is the task a person has in mind.
- **Gets in the way** is what the code makes them do instead, or what it leaves out.
- Each row has an id (`B01` … `B75`) that the other two files refer to.
- "Surface", "Workplace", "Role", "Engine" and "Installed agent" are Bate's own terms (`GLOSSARY.md`). A first-time person has not read that file. Wording problems are listed where the word appears.

## Map of the window

One page: a list of bots on the left, a header with nine tabs, a side pane that opens on the right (`main.tsx:158-277`).

| Part | Where | Scope of what it shows |
|---|---|---|
| Bot list and "New bot" box | left, 14 rem wide (`style.css:5`) | all bots |
| Header: bot name and nine tabs | top of the main area (`main.tsx:205-212`) | the chosen bot |
| Chat | tab 1 | the chosen bot |
| Routines, Connectors, Skills | tabs 2, 3, 5 | the chosen bot (`main.tsx:236-237,250`) |
| Memory | tab 4 | the chosen bot, and its project's notes below (`main.tsx:238-249`) |
| Groups | tab 6 | all groups (`main.tsx:251`) |
| Projects, with Workplaces and the catalog under it | tab 7 | all projects, all workplaces (`main.tsx:252`) |
| Room | tab 8 | the chosen bot's project (`main.tsx:253`) |
| Settings | tab 9 | 5 sections for the bot, 6 for the whole app (B63) |
| Side pane (a bot's file, a surface) | right | the chosen bot or project (`main.tsx:260-265`) |
| Command palette | ⌘K | commands, bots, words from chats (`Palette.tsx`) |

The window opens at 1000 × 700 (`window/src/main.rs:101`).

## 0. The frame: bot list, header, tabs

| Id | Tries | Gets in the way | Evidence |
|---|---|---|---|
| B01 | Open Projects, Workplaces, the catalog, Devices or sign-in right after install | With no bot chosen the main area says "Choose a bot." or "Create a bot to begin." and nothing else can be opened. Everything that is not a bot is a tab of a bot. | `main.tsx:257`, `main.tsx:203-256` |
| B02 | Know what a tab belongs to | Nine tabs sit in one row, with three scopes: the bot (Chat, Routines, Connectors, Memory, Skills, Settings), a project (Room, half of Memory), nothing in particular (Groups, Projects). The label does not say which. Switching bot keeps the tab, so Projects looks like "this bot's projects". | `main.tsx:26`, `main.tsx:251-253`, `Projects.tsx:24` |
| B03 | Switch tab while reading a long page | The header is the first child of `main`, which scrolls, and is not sticky. On Settings (about 180 lines of form) or Projects the tabs scroll away. | `style.css:12-13`, `main.tsx:198-212` |
| B04 | Move between tabs with the keyboard | The tabs are buttons with `role="tab"` and `aria-selected`. No arrow-key handling, no `tabpanel`. The chosen tab differs from the rest by bold weight only. | `main.tsx:207-211`, `style.css:14` |
| B05 | See the result of an action | No common place for feedback. Each screen draws its own red line at the end of its own page. The app-wide one goes away only when clicked, and nothing says so. Colour `#d33` is the only sign that it is an error. | `main.tsx:202`, `style.css:16`, `Projects.tsx:173`, `Settings.tsx:181` |
| B06 | Understand why nothing loads | While the service is not there the page shows one grey line, "Connecting to the service…" or "The connection failed: …" and the raw error. No Retry, no hint what to do. A failed request shows the raw text, for example "the service restarted". | `main.tsx:30,79,199`, `service.ts:122` |
| B07 | Learn that a bot other than the open one needs an answer | The bot list shows only " · working" for a busy bot. A bot that waits for the person's answer is busy too, so it looks like one that is thinking. No badge, no sound, no macOS notification (the shell has no notification code). | `main.tsx:188`, `Chat.tsx:379-389`, `window/src/main.rs` (searched `notif`: none) |
| B08 | Find one bot among many, or see which belong together | The list is flat: no search, no grouping by project (each bot knows its project), no unread mark, no last message. Reorder is by drag, or by Alt+↑/↓ that is announced only in a tooltip. | `main.tsx:160-192`, `main.tsx:178`, `types.ts:25` |
| B09 | Make a bot | One text box and a Create button at the foot of the list; ⌘N only moves focus into the box. The bot starts with a name and nothing else. Instructions, model and avatar are in the Settings tab, and no message says so. After Create the person lands on an empty chat. | `main.tsx:86-95,126,193-196`, `Settings.tsx:68-87` |
| B10 | Jump to anything with ⌘K | The palette lists bots, the tabs of the chosen bot, "Find in chat", "New bot", "New window" and chat hits. It has no entry for a project, a group, a workplace, sign-in, Devices or Updates. A search with no hits shows an empty box. A chat hit carries the bot and "you" or "reply", no date. | `main.tsx:149-155`, `Palette.tsx:47,67,84-97` |
| B11 | Drive the window with keys | Four fixed keys: ⌘K, ⌘F, ⌘N, ⇧⌘N. None for Stop, next or previous bot, a tab, Settings, or "send" in the Room. | `main.tsx:133`, `main.tsx:152-154` |
| B12 | Use the page on a phone or tablet (it is served to other devices) | No `@media` rule: a 14 rem list, nine tabs in a flex row that does not wrap, a pane that floats over the chat below 776 px (196 + 340 + 240). ChatGPT sign-in, Channels, Devices, Helper and the first-run box are left out on other devices without a word. | `style.css` (searched `@media`: none), `style.css:13`, `pane.ts:4-13`, `main.tsx:201`, `Settings.tsx:131,167-169` |
| B13 | See where the keyboard focus is; read in the dark | No `:focus` or `:focus-visible` rule except on the pane's grip; no reduced-motion rule; colours are system keywords (`Highlight`, `Canvas`) plus fixed values (`#d33`, `#f5a623`, `white`). Dark mode works through `color-scheme`; nothing is designed for it. | `style.css:3,16,48,61,65` |

## 1. First run and sign-in

What a new person sees: the bot list with a "New bot" box, the main area saying "Create a bot to begin.", and above it a "Welcome to Bate" box.

| Id | Tries | Gets in the way | Evidence |
|---|---|---|---|
| B14 | Sign in so a bot can answer | The box says to open "a bot's Settings", then find "ChatGPT" or "Model server". With no bot it says "Create a bot on the left first." That is three jumps before the sign-in button: create a bot, open Settings, scroll past "The bot" and "What the bot may do" (about 60 lines of form) to "ChatGPT". | `FirstRun.tsx:25-30`, `Settings.tsx:71-133` |
| B15 | Dismiss the box and come back to it | "Not now" is saved for ever in `localStorage`. After it nothing reminds the person. A bot with no model then fails with the service's raw message after "The reply failed:". | `FirstRun.tsx:12,33`, `Chat.tsx:559` |
| B16 | Try Bate on a second device first | The box is drawn in Bate's own window only. A phone sees "Create a bot to begin." and nothing about sign-in. | `main.tsx:201` |
| B17 | Send a first message | No first bot, no example prompt, no "say hello" step. The chat of a new bot is a blank area (B20). | `main.tsx:257`, `Chat.tsx:339-378` |
| B18 | Know that macOS wants an approval for the service | The shell's note appears above the page in the same grey style as every other note. | `main.tsx:50-56,200` |

## 2. Pairing screen (another device only)

| Id | Tries | Gets in the way | Evidence |
|---|---|---|---|
| B19 | Pair a phone with the Mac | The screen says to open "a bot's Settings and choose Pair a device". That button is at the foot of Settings under "Other devices" and shows only after "Let devices on this network open Bate" is ticked. The Mac side needs four steps (bot, Settings, switch, button); the phone screen names the first two only. | `Devices.tsx:45-47,64-65,99` |

## 3. Chat tab

| Id | Tries | Gets in the way | Evidence |
|---|---|---|---|
| B20 | Start talking to a new bot | A bot with no messages draws an empty area. No empty state, no example, no loading state while the history loads. (The Room has "Nothing said here yet.") | `Chat.tsx:339-378`, `Room.tsx:39` |
| B21 | Read the conversation | Above the first message sit up to four rows: the model form, "Messages with other bots" (when any), the surfaces row (when any) and "Export chat". | `main.tsx:214-223`, `Chat.tsx:333-338`, `Mail.tsx:22` |
| B22 | Choose a model | Pick in a select, then press "Use": two steps. "Thinking" is disabled until a model is chosen and does not say why. "the engine's default" does not name the model. When the server lists none, the field is a free text box, and a typo shows on the next reply. The row is hidden for installed agents. | `Model.tsx:48-55,61,66`, `main.tsx:214` |
| B23 | See what the bot is doing | The only signs: " · working" in the bot list, and Send turning into Queue, Steer and Stop. No spinner, no elapsed time, no "step 3 of 5" unless the bot writes a plan. Thinking text is always shown, dim and italic, in the reply's column. A tool call reads "run done" and opens to raw JSON. | `main.tsx:188`, `Chat.tsx:463-475`, `Chat.tsx:518-538`, `style.css:24` |
| B24 | Stop a bot that goes wrong | Stop is the last button of a row (Compact, Queue, Steer, Stop) and has no key. "Queue", "Steer" and "Compact" are explained in tooltips only. | `Chat.tsx:460-472` |
| B25 | Answer a risky question | The question card sits between the list and the composer: title, target, and buttons that carry the service's own words. All buttons look alike; nothing marks "allow always" or "deny" as different. It exists only while that bot's chat is open (another tab or bot unmounts the chat). | `Chat.tsx:379-389`, `main.tsx:224-235` |
| B26 | Find yesterday's conversation | Each bot has one conversation without an end. No "new chat", no list of past chats, no titles, no dates. Only ⌘F (inside the bot) and ⌘K (all bots) remain, and a hit shows a snippet without a date. | `Chat.tsx:10,79-95`, `Palette.tsx:33,47` |
| B27 | Go back and try again | "Fork" does not start a new chat: it rewinds the same chat and greys what followed ("The bot goes on from before the greyed messages above"). The tooltip is the only explanation. | `Chat.tsx:263-268,510,563` |
| B28 | Keep a copy of a chat | "Export chat" is on screen all the time and answers with a file path as text; no "Show in Finder". | `Chat.tsx:334-337` |
| B29 | Read what a tool did | "`run` done" and the JSON of the arguments and the raw output. No one-line summary such as the command that ran. | `Chat.tsx:530-538` |
| B30 | Send a message with files | Attachments become lines of text (`Attached:` and the paths) added to the person's own message, so the bubble then shows the paths. | `Chat.tsx:236-237` |

## 4. Routines tab

| Id | Tries | Gets in the way | Evidence |
|---|---|---|---|
| B31 | Learn what a routine is | No intro line, no empty state, no example. A new bot shows an empty list and a "New routine" form. | `Routines.tsx:73-94` |
| B32 | Set "every morning at 9" | The form reads "Every [1] [days] From [09:00]". The list prints "every 1 days from 09:00" (the plural is not adjusted). No time zone, no next run shown before saving. | `Routines.tsx:97-104`, `Routines.tsx:82` |
| B33 | Run a routine now to try it | No "Run now" in the window. Past runs are in a `<details>` per routine. | `Routines.tsx:33,76-91` |
| B34 | Use the web address of a routine | It is printed as plain code with no copy button; the check box that makes it says "Also run from a web address". | `Routines.tsx:87,105` |
| B35 | Remove a routine | "Delete" acts at once; no confirm. (Skills, Groups, Projects, Memory and Workplaces ask first.) | `Routines.tsx:89` |

## 5. Connectors tab

| Id | Tries | Gets in the way | Evidence |
|---|---|---|---|
| B36 | Add a tool server | No intro line, no empty state, no example. Fields: Name (a pattern shown only as a tooltip), "Reached: over the web / by running a program", Address or Command, "The server signs me in". | `Connectors.tsx:39-77`, `Connectors.tsx:60,64,71` |
| B37 | Type a command with arguments | The command is split at spaces, so an argument with a space cannot be written. The code says so in a comment; the screen does not. | `Connectors.tsx:28-29` |
| B38 | Know that a connector works | A row shows name, address, a switch, Sign in and Delete. No "connected" mark, no tool count, no error from the last start. | `Connectors.tsx:41-56` |
| B39 | Remove a connector | "Delete" acts at once. | `Connectors.tsx:54` |
| B40 | Give three bots the same connector | Connectors are per bot; the form is filled in once per bot. | `main.tsx:237`, `Connectors.tsx:7` |

## 6. Memory tab

| Id | Tries | Gets in the way | Evidence |
|---|---|---|---|
| B41 | See what the bot remembers | A list of files with size and date, a "Reload" button (the bot's own writes are not announced, so the list can be stale) and an editor under it. | `Memory.tsx:43-47,64,69-72` |
| B42 | Tell the bot's notes from the project's | For a bot in a project two lists stack; the second has a heading, the first has none. | `main.tsx:238-249` |
| B43 | Write a note | "Name" must be a file name ending in `.md`, at most 100 bytes, no leading dot, no `/`: one long grey sentence. While a note is being edited no other note can be opened. | `Memory.tsx:88-93`, `Memory.tsx:71,98` |

## 7. Skills tab

| Id | Tries | Gets in the way | Evidence |
|---|---|---|---|
| B44 | Add a skill | Type the full path of a folder (`/Users/you/Downloads/my-skill`); `~` is refused with a message after the click. No folder picker, no catalog. | `Skills.tsx:25,55` |
| B45 | Know what a skill is | One paragraph of rules: a `SKILL.md`, a `---` block, limits of 500 files and 20 MiB. | `Skills.tsx:51-53` |
| B46 | Install the same skill on every bot | Skills are per bot; each bot is a separate install. A skill with the same name is replaced without a question. | `main.tsx:250`, `Skills.tsx:52` |

## 8. Groups tab

| Id | Tries | Gets in the way | Evidence |
|---|---|---|---|
| B47 | Make several bots work together | Two screens claim it: Groups ("Bots in a group can message each other, and the lead hands out the work.") and Projects ("The bots in a project work in the project's folder and can message each other."). Nothing says which to use. | `Groups.tsx:38`, `Projects.tsx:65` |
| B48 | Talk to a group | No screen writes to a group. `Groups.tsx` is the only file in `window/ui/src` that handles groups (grep `Group`). A person can make one and cannot address it. | `Groups.tsx:7-85`, `main.tsx:251` |
| B49 | Make a group of one | With fewer than two bots the form is replaced by a sentence. | `Groups.tsx:59` |

## 9. Projects tab (with Workplaces and the catalog)

| Id | Tries | Gets in the way | Evidence |
|---|---|---|---|
| B50 | Start a project | The tab is inside a bot, though a project is not part of a bot; with no bot there is no way in. It lists every project whichever bot is chosen. | `main.tsx:252`, `Projects.tsx:24`, `GLOSSARY.md` ("Project") |
| B51 | Fill in the project form | One long form: name, folder, instructions, hosts, workplace, connector, bots, lead; eight groups of fields and one button at the end. | `Projects.tsx:96-171` |
| B52 | Choose a folder | A text box. The placeholder is `~/code/my-site`; the note under it says "The folder's full path"; the Skills screen says the service does not expand `~`. No folder picker. | `Projects.tsx:101-103`, `Skills.tsx:24` |
| B53 | Edit a project | "Edit" under a row loads the form at the foot of the page, below every row; the page does not scroll and focus does not move. | `Projects.tsx:82,96-98` |
| B54 | Add bots to the project | Tick boxes for every existing bot. A bot already in another project is greyed with "(in X)" and cannot be moved. New bots come from "Add bots from role files": a `<details>` inside each row, a textarea of whole file paths, then "Show what they create". | `Projects.tsx:152-159`, `Projects.tsx:194-200` |
| B55 | Open the project and talk to it | A project row has Edit and Delete, not "Open" or "Room". To write to the project's bots: remember a member bot, pick it, open the Room tab. | `Projects.tsx:69-95`, `main.tsx:253` |
| B56 | Pick the lead | "Lead, who gets a room message with no @name", default "The first bot". | `Projects.tsx:164-168` |
| B57 | Add a program for the project | The form carries a developer paragraph that names `BATE_MCP_SOCKET`, and the rule "A project has a workplace or a connector of its own, not both." | `Projects.tsx:140-149,137` |
| B58 | Install a workplace | A typed folder path, or the catalog behind a button because reading it "contacts GitHub". The preview before install (what it needs, sizes, hosts) and "Nothing is downloaded until you agree" are good. The place is not: workplaces sit under the project form, under a bot's tab. | `Workplaces.tsx:104-106,120-130,131-140`, `Projects.tsx:174` |

## 10. Room tab

| Id | Tries | Gets in the way | Evidence |
|---|---|---|---|
| B59 | Write to the project's bots | A plain textarea and a "Send" button; Enter does not send (the Chat box does). `@name` is typed by hand with no suggestions; the member names are listed in a sentence under the box. A name with a space is ambiguous. | `Room.tsx:40-47`, `Chat.tsx:454` |
| B60 | Read the replies | Plain text with `pre-wrap`: no Markdown, no code blocks, no tool rows, no "working" mark, no Stop, no attachments. The list has no scroll area of its own and does not follow new lines. | `Room.tsx:27-38` |
| B61 | Reach the room from a bot that has no project | The tab says "This bot is in no project, so it has no room." and offers nothing next. | `main.tsx:253` |
| B62 | Know how a bot's own Chat relates to the Room | A bot in a project has both; nothing says which to use for what. | `main.tsx:224,253` |

## 11. Settings tab

| Id | Tries | Gets in the way | Evidence |
|---|---|---|---|
| B63 | Find a setting | One scroll of 11 sections: The bot, What the bot may do, ChatGPT, Model server, Engine, Bug report, Channels, Other devices, Bate Helper, Updates, Delete. Six belong to the whole app (ChatGPT, Model server, Bug report, Other devices, Helper, Updates); five to the chosen bot. | `Settings.tsx:71,88,133,145,159,164,171`, `Channels.tsx:33`, `Devices.tsx:34`, `Helper.tsx:21`, `Updates.tsx:19` |
| B64 | Know that Save worked | `Save` clears an error and shows nothing. Errors appear at the very end of the page, far from the button that was pressed. | `Settings.tsx:65,86,181` |
| B65 | Set what the bot may do | Seven selects (always, ask me, never) with no explanation of the levels. A change applies at once with no confirm, including "always" for running commands. "always" is missing for one kind with no reason shown. | `Settings.tsx:10-18,89-99` |
| B66 | Limit the bot's disk and network | Two check boxes disable each other; the reason sits in a long note that shows only once one is ticked. | `Settings.tsx:114,117,123` |
| B67 | Sign in, or use another model server | ChatGPT and a Model server are two sections one after the other. The rule "ChatGPT wins" is one sentence ("Signed in: every bot runs on your ChatGPT plan, not on the model server below."). Both are app-wide but shown inside a bot, and only in Bate's own window. | `Settings.tsx:133-157` |
| B68 | Pick an engine | "Installed agent (empty: Bate's own)" asks for a command such as `opencode acp`. | `Settings.tsx:159-163` |
| B69 | Make the bot look like itself | Avatar is one typed emoji; colour is the system input plus "No colour"; "Tool rounds per turn" is a number with a hint. No preview. | `Settings.tsx:77-85` |
| B70 | Remove something safely | Delete bot asks first and says what happens ("move its folder to the Trash"). Removing an always-allowed grant, Sign out of ChatGPT, Remove key, Remove device, Remove a channel token and Remove a chat act at once. | `Settings.tsx:171-180` against `Settings.tsx:107,137,154`, `Devices.tsx:75`, `Channels.tsx:60,90` |
| B71 | Connect Telegram or Discord | A password box labelled "Telegram bot token" with no steps for getting a token. The pairing code with a countdown is good. | `Channels.tsx:44-52,63-69` |
| B72 | Give the helper its macOS permissions | One row per permission with "Open System Settings" (good). When the helper is missing the text tells a person to run `helper/bundle.sh`. | `Helper.tsx:24,27-42` |
| B73 | Update Bate | "This build has no update key, so it does not update itself." is a developer's sentence shown to a person. | `Updates.tsx:23` |

## 12. Side pane: a bot's file, a surface

| Id | Tries | Gets in the way | Evidence |
|---|---|---|---|
| B74 | Open what a bot made | A file link in a reply opens in the pane (good). Surfaces show as "name (kind)" buttons above the chat only when a project's program offers any; otherwise the row is not drawn. | `Markdown.ts:25-33`, `Surfaces.tsx:22-27` |
| B75 | Close or resize it | A "Close" button; a drag handle with arrow keys; below 776 px of width the pane floats over the chat. Esc does not close it. | `Pane.tsx:34-54`, `pane.ts:4-13` |

## What the rows add up to

75 rows. Some rows count in two kinds.

| Kind of problem | Rows | Where it hurts most |
|---|---|---|
| Reachable only through an unrelated bot | B01, B02, B19, B50, B55, B58, B63, B67 | Projects, Workplaces, sign-in, Devices |
| No empty state or no first-use guidance | B17, B20, B31, B36, B42, B45, B61 | Chat, Routines, Connectors |
| Forms with no guidance (rules in a grey paragraph, typed paths) | B37, B43, B44, B51, B52, B57, B58, B59, B68 | Skills, Projects, Settings |
| No feedback after an action | B05, B06, B33, B38, B64 | Settings, Routines, Connectors |
| Destructive action with no confirm, or an inconsistent one | B35, B39, B65, B70 | Routines, Connectors, Settings |
| Keyboard use | B04, B11, B24, B59, B75 | tabs, Stop, Room, pane |
| Wording that needs the glossary | B22, B24, B27, B36, B57, B68, B73 | Chat, Projects, Settings |
| State of a running bot, and who needs the person | B07, B23, B25 | the bot list, Chat |
| Finding old work | B10, B26, B27 | Chat, palette |
| Small screens, focus, dark | B12, B13 | the whole page |

## Not checked

- The running window. Everything above is from reading the code. No screen was rendered, clicked or timed. Sizes (the 776 px limit) are computed from `pane.ts`, not measured.
- What the service sends. Texts that come from `appd` (error messages, the question's title and options, the plan) were not read, so B15, B25 and B29 may be better or worse in practice.
- VoiceOver or any screen reader. `aria` attributes were seen in the code, not tried.
- The phone view. B12 is from the CSS only.
- Light against dark. Only the CSS keywords were read.
- Behaviour with many bots or very long chats. Virtual scrolling was read, not stressed.
- The Tauri shell's own menus beyond `window/src/main.rs:117-162` (menu definitions), and whether macOS shows a notification for a turn that ends in the background (the shell has no notification code; the service may).
