Seven open-source bot apps and several closed ones were compared on ten tasks; on each task there is a point where Bate does it differently from what most of them do.

# How other bot apps do the same tasks

Read 2026-11-20. This file answers one question: for each task a person does in Bate's window, how do other apps do it, and what should Bate do? The problems in Bate's window are in [`01-bate-today.md`](01-bate-today.md) (ids `B01`…); the ranked changes are in [`README.md`](README.md) (ids `R1`…).

## Sources and how to read them

| Kind | What was read | How it is cited |
|---|---|---|
| Open-source apps, UI code | Shallow clones of the default branch in `/tmp/refs` (commits in [`../components/README.md`](../components/README.md)): OpenClaw (`openclaw/openclaw`), Hermes (`NousResearch/hermes-agent`), nanobot (`HKUDS/nanobot`), pi (`earendil-works/pi`), omp (`can1357/oh-my-pi`), `CopilotKit/OpenBot`, `nightly-labs/openbot` | `path:line`, relative to the repo's root |
| Closed desktop bot apps | Patterns, described in general words | No name, no code, no screenshot. |
| Public guidance | Apple Human Interface Guidelines (HIG); Nielsen's ten usability heuristics | URL |
| Bate | `window/ui/src/` on this branch | `path:line`, relative to `window/ui/src/` unless it says otherwise |

The other apps in [`../technology/README.md`](../technology/README.md) (ZeroClaw, PicoClaw, NanoClaw, Kwant, Brigade, AionUi) have no clone here and were not read for this file.

Names in the tables: **terminal** apps (pi, omp) have no window; their rows describe the terminal screen and its keys. **Web** means the app's browser UI. Where an app has several UIs, the row names the one read.

Public guidance used in more than one section:

- HIG, [Sidebars](https://developer.apple.com/design/human-interface-guidelines/sidebars): a sidebar leads to "areas of your app or top-level collections of content"; "show no more than two levels of hierarchy"; "avoid putting critical information or actions at the bottom of a sidebar"; let people hide it.
- HIG, [Onboarding](https://developer.apple.com/design/human-interface-guidelines/onboarding): "fast, fun, and optional"; "postpone nonessential setup"; give "reasonable default settings"; keep a skipped tutorial easy to find later.
- HIG, [Settings](https://developer.apple.com/design/human-interface-guidelines/settings): "minimize the number of settings"; open with ⌘, in the App menu; options "that affect only a specific task" belong in the task itself.
- HIG, [Alerts](https://developer.apple.com/design/human-interface-guidelines/alerts) and [Feedback](https://developer.apple.com/design/human-interface-guidelines/feedback): warn "when their action might destroy data" and cannot be undone; do not alert for "common, undoable actions"; show status "near the items it describes".
- Nielsen, [10 usability heuristics](https://www.nngroup.com/articles/ten-usability-heuristics/): visibility of system status (1), match with the real world (2), user control and freedom (3), error prevention (5), recognition rather than recall (6), help users recover from errors (9).

---

## 1. First run and sign-in

**Bate today.** A "Welcome to Bate" box above "Create a bot to begin." It says to open a bot's Settings and sign in there; "Not now" hides it for good; nothing exists on other devices (B14–B18). Evidence: `FirstRun.tsx:25-37`, `main.tsx:201,257`.

| Who | What a person sees | Evidence |
|---|---|---|
| OpenClaw | Native wizard: welcome, where it runs, then "Connect your AI", which reuses access already on the Mac; a "Set up later" link; a last page of next steps. The web UI has an onboarding mode that hides the nav and shows a setup card with a dismiss button. | `apps/macos/Sources/OpenClaw/OnboardingView+Pages.swift:46,94,800`, `apps/macos/Sources/OpenClaw/OnboardingView+AISetupPage.swift:19,47-49`, `ui/src/app/onboarding-mode.ts:2`, `ui/src/pages/custodian/custodian-setup-card.tsx:139,167` |
| Hermes | "Set up Hermes Desktop" (connect or install), then a provider picker with "I'll choose a provider later" (the skip is stored), then a scripted first chat that walks through name, colour, layout and a first task; "Skip setup" sits in that chat's composer. A `default` profile exists from the start. | `apps/desktop/src/components/desktop-install-overlay.tsx:444,454`, `apps/desktop/src/i18n/en_onboarding.ts:4,10,15`, `apps/desktop/src/i18n/en_first_run.ts:5-11`, `apps/desktop/src/app/chat/composer/index.tsx:1318` |
| nanobot | No wizard in the UI. The terminal creates the config and opens a signed-in browser. The first screen is an empty chat. A first message without a model gets "needs a model… Settings → Models". | `docs/webui.md:17`, `nanobot/providers/unconfigured_provider.py:27`, `webui/src/components/settings/SettingsPage.tsx:281` |
| pi | No wizard. With no model the terminal says to use `/login`, which opens the provider list. The web app has a "Sign in" page that takes a one-time token. | `packages/coding-agent/src/core/auth-guidance.ts:8,15`, `packages/coding-agent/src/modes/interactive/interactive-mode.ts:1250-1251`, `apps/p2/src/runtime/page.ts:80` |
| omp | A terminal wizard runs on launch when its version is newer than the stored one: providers, model, glyphs, composer, theme, updates. The sign-in scene has Skip, which becomes Continue after a sign-in. `omp setup` runs it again. | `packages/tui/src/setup/wizard.ts:18,44,52`, `packages/tui/src/setup/scenes/sign-in.ts:28,195`, `packages/coding-agent/src/main.ts:700-711` |
| `CopilotKit/OpenBot` | Not signed in goes to a sign-in page; not onboarded goes to a wizard of at most three steps (welcome, an optional step with "Not now", a name step ending in "Get started"). No provider step; setup is a checklist in Settings. A built-in assistant exists, and the empty roster says "Create a Bot for a specific job". | `app/src/routes/_authed.tsx:41,50`, `app/src/routes/_authed/onboarding.tsx:89-91,125,185`, `app/src/components/bots/bot-roster.tsx:112,124-128` |
| `nightly-labs/openbot` | One question first: "Use free models" or "Use my AI subscription"; the free path needs no sign-in; the subscription path has "Use free models instead" as its way out. Then, with no agent yet, the "Create your first agent" form opens by itself. | `packages/i18n/src/messages/en/onboarding.ts:80-84,95`, `src/renderer/src/features/onboarding/FirstRunChoice.tsx:59,72,92`, `src/renderer/src/features/agents/agents-context.tsx:268-271` |
| Pattern seen in closed desktop apps | A short guided first run before the main window. A step that needs a macOS permission sends the person to System Settings. | — |
| HIG | Onboarding should be "fast, fun, and optional"; "postpone nonessential setup"; ask for a permission "when people first access the specific function". | [Onboarding](https://developer.apple.com/design/human-interface-guidelines/onboarding) |

**Most share.** A path that ends at a chat that works. Sign-in or provider setup is part of the first-run flow in OpenClaw, Hermes and `nightly-labs/openbot`, and each offers a way past it ("Set up later", "I'll choose a provider later", "Use free models"). nanobot and `CopilotKit/OpenBot` leave it for Settings, but the empty or broken state points back to it.

**Differ.** Whether a first bot is made for the person (Hermes `default`, `CopilotKit/OpenBot` built-in assistant: yes; `nightly-labs/openbot`: the form opens at once; OpenClaw's UI: not seen). Whether the skip is remembered (Hermes: yes).

**Recommendation for Bate (R2).** Sign in on the first screen, with a stored "Set up later". Create a first bot, so the chat is usable at once. When a bot is asked to answer with no model, say so in the chat with a "Sign in" button (nanobot's pill that turns into setup; `webui/src/components/thread/ThreadComposer.tsx:1935,2320`). Keep "ChatGPT or a model server" as one question, as in `nightly-labs/openbot`. Never ask for a bot's Settings first.

---

## 2. Making a bot

**Bate today.** A name box and Create at the foot of the list; everything else is in the Settings tab (B09, B68–B69). Evidence: `main.tsx:86-95,193-196`, `Settings.tsx:68-87`.

| Who | What a person sees | Evidence |
|---|---|---|
| OpenClaw | "Create new agent" in the sidebar menu and a button on the Agents page. One dialog: name (max 64) and an avatar picker with three tabs (preset, generate, upload). Create stays disabled until there is a name. No model field. It opens the new agent's chat. | `ui/src/components/app-sidebar-agent-menu.tsx:265,268,349`, `ui/src/components/agent-create-dialog.tsx:70,151-155,161`, `ui/src/pages/agents/avatar-picker.tsx:36,193` |
| Hermes | A "+" in the profile rail opens "New profile": name, "Clone from" (default `default`) and an optional persona text. No avatar, no model at creation. It then switches to a fresh chat of the new profile. | `apps/desktop/src/app/chat/sidebar/profile-switcher.tsx:668,693-695`, `apps/desktop/src/app/profiles/create-profile-dialog.tsx:104,113,115,127` |
| nanobot | No bot creation in the UI; the sidebar has "New topic" only. A second bot is a second instance started from the terminal. | `webui/src/components/Sidebar.tsx:134`, `docs/multiple-instances.md:44-45` |
| pi | No bot object. The unit is a session; `/new` starts one. The web app's "New session" asks for a folder, an optional project and a model. | `packages/coding-agent/src/core/slash-commands.ts:39`, `apps/p2/src/web/new-session.tsx:14-23,56` |
| omp | `/agents` opens a hub whose last row is "+ New agent…"; one text box: "describe it and let the architect draft it". No avatar. | `packages/tui/src/overlays/agents-hub.ts:362,1042,1080`, `packages/coding-agent/src/slash-commands/builtin-session.ts:591-593` |
| `CopilotKit/OpenBot` | "New Bot" in the Bots page header and in the empty roster. A right-hand sheet: name, optional instructions, the apps it may use (ticked by default). The avatar is generated, with no picker. No model field. | `app/src/routes/_authed/_app/bots.index.tsx:57,69`, `app/src/components/agents/new-bot-sheet.tsx:118,225,536,579` |
| `nightly-labs/openbot` | A "+" menu next to sidebar search, a button in the empty sidebar, or automatically when there is no agent. One form in the main area: face, colour, name (required), "What should this agent help with?", model. Six suggestion cards fill the fields. | `packages/ui/src/features/sidebar/SidebarSearch.tsx:34,45`, `packages/ui/src/features/agents/FirstAgentSetup.tsx:139-199,528,537-542,548-552`, `packages/i18n/src/messages/en/agent.ts:7,18-21` |
| Pattern seen in closed desktop apps | A bot is made from a name, a look and a purpose, or from a ready-made template. | — |

**Most share.** One step, a short form with a name and a prompt-like purpose. Five of seven need no model choice at creation. The entry point is the sidebar (OpenClaw, `nightly-labs/openbot`), a page button (`CopilotKit/OpenBot`) or a rail (Hermes).

**Differ.** Avatar picker only in OpenClaw and `nightly-labs/openbot`; starter cards only in `nightly-labs/openbot` (OpenClaw has a separate `ui/src/app-route-paths.ts:56` catalog); omp has the model draft the prompt.

**Recommendation for Bate (R2).** Keep one step, but make it a form with name, purpose (the instructions' first line), avatar and starter cards, as in `nightly-labs/openbot` (`packages/ui/src/features/agents/FirstAgentSetup.tsx:139-199`). Put the entry in a "+" at the top of the list, not at the foot (HIG: "avoid putting critical information or actions at the bottom of a sidebar"). After Create, open the chat with a first-message suggestion.

---

## 3. Starting a project and adding bots to it

**Bate today.** Projects are a tab of a bot; the form has eight groups of fields; bots are ticked from the list; a project has no page and no "Open" (B50–B58). Evidence: `main.tsx:252`, `Projects.tsx:96-171,152-159`.

| Who | What a person sees | Evidence |
|---|---|---|
| OpenClaw | No project above agents; the agent is the top object, with its own workspace. The "New session" page has a project chip (folders, recents) and an agent select. A cloned project always starts in a managed worktree. | `docs/concepts/multi-agent.md:87-88,124`, `ui/src/pages/new-session/target-controls-view.tsx:22-29,132-139`, `docs/concepts/managed-worktrees.md:13-16` |
| Hermes | A project is a name plus folders, made in a dialog that opens a folder picker (the folder names the project if the name is empty). It sits in a "Projects" section of the sidebar. Bots (profiles) are in a separate "Bots" tab, so bots are not added to a project; group chats are a third object. | `apps/desktop/src/app/chat/sidebar/project-dialog.tsx:138,163,170`, `apps/desktop/src/i18n/en_projects.ts:8-10`, `apps/desktop/src/plugins/hermes-bots/plugin.tsx:4,450-452` |
| nanobot | No project object. A folder attached to a chat through a picker in the composer (browse, type a path, recents); the sidebar then groups topics under "Projects" by folder. | `webui/src/components/thread/WorkspaceControls.tsx:158,224-225,427`, `webui/src/lib/chat-groups.ts:44-45` |
| pi | The terminal groups sessions by the folder where they began (`/resume` lists "Current Folder" or "All"). The web app has projects: "New project…" picks a folder and a name, and a session joins a project by starting from its row. | `packages/coding-agent/docs/sessions.md:14,16,66`, `apps/p2/README.md:237-249,257` |
| omp | No project object. A session is tied to its folder; `/resume` has current-folder and all-projects scopes. | `docs/session-operations-export-share-fork-resume.md:419-422,434` |
| `CopilotKit/OpenBot` | No project or folder. Bots are chosen in a "To:" field when a conversation starts: one bot is a chat, two or more (up to 20) are a group. | `app/src/routes/_authed/_app/channel/new.tsx:48,281`, `app/src/components/channels/compose-state.ts:14,16` |
| `nightly-labs/openbot` | No project. Sidebar "sections" are named folders that agents are dragged into (a name is the only field). "Channels" are shared chats made from a name and a searchable list of agents; purpose and lead come later in settings. | `packages/ui/src/features/sidebar/SidebarNav.tsx:110`, `src/renderer/src/features/channels/ChannelCreateDialog.tsx:24,95,109,167` |
| Pattern seen in closed desktop apps | A "project" is a label or a sidebar section that groups bots; it is not always a folder. | — |
| HIG | A sidebar leads to "top-level collections"; "no more than two levels"; use disclosure for depth. | [Sidebars](https://developer.apple.com/design/human-interface-guidelines/sidebars) |

**Most share.** Work is grouped by a folder or a label, and the bots stay top-level, each with its own place. Only Hermes has a real project object, and even there bots are not members of it.

**Differ.** Bots join a group at creation by a picker in `CopilotKit/OpenBot` and `nightly-labs/openbot`; a project is created from a folder picker in Hermes and pi's web app, and from a typed path in nanobot's picker.

**Recommendation for Bate (R1, R6).** Bate's glossary already says a project is a folder the person and its bots share, so Bate does not need to copy "bots are not in projects". Make Projects a top-level section of the sidebar above Bots (#142). Create a project from a folder picker (the folder names it, as in Hermes). Add bots to it from a searchable list in the same dialog, or "New bot in this project"; leave purpose and lead for later, as in `nightly-labs/openbot` (#147). Give a project a page: Room, bots, notes, workplace. ADR 0002 answers whether a bot may be in no project or in two: none or one.

---

## 4. Talking to one bot and to several at once

**Bate today.** One endless chat per bot (Chat tab); a Room tab for a project with a plain box where `@name` is typed by hand; Groups that nobody can write to (B20, B47–B48, B59–B62). Evidence: `Chat.tsx:418-476`, `Room.tsx:40-47`, `Groups.tsx:38`.

| Who | What a person sees | Evidence |
|---|---|---|
| OpenClaw | One bot: an agent select on "New session", "Open chat" per agent, an agent menu in the quick-chat. Several: no group chat in the UI; a coordinator agent hands work to others and agents message each other. One session per agent. | `ui/src/pages/new-session/target-controls-view.tsx:22-49`, `docs/concepts/multi-agent.md:87-88,124-126`, `apps/macos/Sources/OpenClaw/QuickChatController.swift:579-590` |
| Hermes | One bot: click its row. Several: a group chat of 2–6 bots; "@" lists the handles; "@all" is reserved; the placeholder reads "Message the group — @mention to assign". A message that names nobody follows a setting: everyone, the lead, or an automatic pick. One transcript with a live chip per member. Panes can be split. | `apps/desktop/src/plugins/hermes-bots/plugin.tsx:5-6,13-14,140-145`, `apps/desktop/src/plugins/hermes-bots/canonical-group-locales.ts:221,239-243,253-254`, `docs/canonical-groups.md:35,44` |
| nanobot | One bot only. "@" lists apps and earlier conversations. Sub-agents that the bot starts appear in a task dock with a Stop button each. | `webui/src/components/thread/ThreadComposer.tsx:2597`, `webui/src/components/thread/SubagentTaskDock.tsx:147-151,236` |
| pi | One agent per session. The example sub-agent extension runs tasks as separate processes (at most 8, 4 at once); in the web app sub-agent conversations nest under their parent. | `packages/coding-agent/examples/extensions/subagent/README.md:7,9,180`, `apps/p2/README.md:257,292-293` |
| omp | Alt+A opens a roster of sub-agents with status and cost; Enter shows that agent's session where you type to steer it, Esc returns; a pinned "Subagents" block sits above the editor. | `docs/agent-hub.md:11,63,73-80,86` |
| `CopilotKit/OpenBot` | A "To:" field with chips: one chip is a chat, two or more a group. In a group "@Name" asks one bot; otherwise every bot answers in turn. In a single chat the "@" is not routed, and the code says so. One shared transcript, each line named. | `app/src/components/channels/group-chat.tsx:41-46,169`, `app/src/components/channels/channel-chat.tsx:2009-2011`, `docs/architecture.md:184,189` |
| `nightly-labs/openbot` | A channel: the "@" picker offers each member and "@everyone". "@A @B" goes to both with no routing turn. With no mention the lead's model picks, and a visible line says who. One chat, each author named. | `packages/ui/src/features/conversation/ComposerEditor.tsx:165-166`, `docs/architecture/channels-and-messaging.md:197,218,223,247,253-258` |
| Pattern seen in closed desktop apps | Group conversations of several bots in one timeline. An "@" menu lists bots and other things the person can name. The person can choose who answers a message that names nobody. | — |

**Most share.** "@name" aims a message at one bot; the person's own words go in one timeline; a sub-agent's view is a panel inside the same chat.

**Differ.** Three apps (Hermes, `CopilotKit/OpenBot`, `nightly-labs/openbot`) have a group chat with one timeline; OpenClaw keeps one session per agent; nanobot, pi and omp have one main bot with sub-agents. The rule for an un-named message differs: everyone, the lead, or an automatic pick.

**Recommendation for Bate (R7).** Make the Room a chat as good as the bot's: Enter sends, "@" opens a list of members and the result becomes a chip, replies are Markdown, a working mark and Stop per bot (#148). Show the lead's rule as a visible line (`nightly-labs/openbot`) and make it a setting of the project (Hermes). Groups, the second way to group bots, are removed (R14, #152; ADR 0002).

---

## 5. Choosing a model

**Bate today.** A form row above the chat: select, then "Use"; "Thinking" disabled until a model is chosen; app-wide sign-in elsewhere (B22). Evidence: `Model.tsx:48-66`, `main.tsx:214`.

| Who | What a person sees | Evidence |
|---|---|---|
| OpenClaw | A drop-up pill in the composer, per session; its tooltip says "This session / Agent default / Global default". Search, provider groups, favourites; an effort gauge beside it. With no model a banner above the composer has a setup button; an empty catalogue says "Models unavailable" with Retry. | `ui/src/pages/chat/components/chat-model-picker.tsx:172-175,214-220`, `ui/src/pages/chat/components/chat-effort-picker.tsx:166`, `ui/src/i18n/locales/en-model-controls.ts:82-87,108`, `ui/src/pages/chat/chat-model-setup.ts:34-60,94-106` |
| Hermes | A pill in the composer with a hotkey; a dialog "Filter providers and models…" with a "Current:" line; the dropdown also sets reasoning effort. A manual pick is sticky for new chats and shows a dot. A spinner while it loads; with no provider, "No authenticated providers." and an "Add provider" button. | `apps/desktop/src/app/chat/composer/model-pill.tsx:123-130,149,186`, `apps/desktop/src/components/model-picker.tsx:206-207,298-301,312-322`, `apps/desktop/src/i18n/en.ts:4037-4044` |
| nanobot | A pill in the composer lists configured presets; picking one sends `/model <name>` for that chat; a "Manage models" row opens settings. With no model the pill becomes a setup prompt and Send becomes a setup action. | `webui/src/components/thread/ModelPresetBadge.tsx:394-480,569-602`, `webui/src/components/thread/ThreadShell.tsx:831-834` |
| pi | Ctrl+L opens a searchable selector (Tab: all or scoped; Ctrl+S: make default); Shift+Tab cycles the thinking level; the footer shows "model • thinking level". With no model: "No models available… Use /login". | `packages/coding-agent/src/modes/interactive/components/model-selector.ts:36,98-114,141-146`, `packages/coding-agent/src/core/keybindings.ts:33,41-49`, `packages/coding-agent/src/modes/interactive/components/footer.ts:233-238` |
| omp | Alt+P opens a session-only drop-up; Alt+←/→ moves a thinking-level slider; `/models` is a full-screen hub for role defaults. An empty list says "No models available". | `packages/tui/src/overlays/model-picker.ts:1-8,635`, `packages/tui/src/overlays/model-hub.ts:1-10`, `packages/tui/src/app-keybindings.ts:133-140` |
| `CopilotKit/OpenBot` | An icon in the composer opens a searchable combobox; a "Thinking" column appears for models that think. Saved per person and conversation. It hides itself when nothing can be chosen; if a pick is withdrawn, a notice names the replacement. | `app/src/components/channels/composer/model-picker.tsx:106-128,129-203`, `app/src/lib/models/run-model.ts:25-60`, `app/src/components/channels/channel-chat.tsx:1911-1918` |
| `nightly-labs/openbot` | A picker in the conversation header and in the agent's settings (per agent), with a provider rail, search and the effort label in the trigger; empty state "No models are available from {name}." | `packages/ui/src/features/conversation/ConversationHeader.tsx:207`, `packages/ui/src/components/ProviderModelPicker.tsx:212,630-651,1259`, `packages/i18n/src/messages/en/provider.ts:136` |
| Pattern seen in closed desktop apps | A model chosen per bot, with a default in the settings. | — |

**Most share.** A pill or trigger next to the composer, a searchable list, effort set next to the model in six of seven (nanobot shows it in a tooltip only).

**Differ.** Scope: per session (OpenClaw, omp), sticky for new chats (Hermes), per chat (nanobot), per conversation (`CopilotKit/OpenBot`), per agent (`nightly-labs/openbot`). No-model state: banner (OpenClaw), the pill turns into setup (nanobot), text hint (pi), picker hidden (`CopilotKit/OpenBot`).

**Recommendation for Bate (R5).** Move the model to a chip in the composer; apply on pick; effort in the same popover, never disabled without a reason. Keep Bate's per-bot scope (it is the glossary's: "its own … permissions") and say so in the tooltip as OpenClaw does ("This bot / App default"). With no model, the chip says "Sign in" and opens the sign-in (R2). When a plan changes the list, say which model replaced a withdrawn one.

---

## 6. Seeing what a bot is doing, and stopping it

**Bate today.** " · working" in the list; Send becomes Queue, Steer and Stop; no spinner, no step, no sound (B07, B23, B24). Evidence: `main.tsx:188`, `Chat.tsx:460-475`.

| Who | What a person sees | Evidence |
|---|---|---|
| OpenClaw | The composer says "{name} is working"; Stop has a tooltip with its Esc key; the send button turns into Queue or Steer; a queue list with reorder and edit, paused by Stop with Resume and Clear; a progress panel with steps; sidebar rows have a running mark and badges for approval, question and error; browser push is set up. | `ui/src/i18n/locales/en-chat-composer.ts:21`, `ui/src/pages/chat/components/chat-composer-controls.tsx:299-320,336-342`, `ui/src/pages/chat/components/chat-composer-queue.tsx:198-228`, `ui/src/components/app-sidebar-session-row-render.tsx:226-234` |
| Hermes | Stop replaces Send when busy; Esc cancels; Enter steers, Cmd+Enter queues; "N Queued — paused"; a stack above the composer shows todos, sub-agents and background work; sidebar marks for working, needs-input, unread, stalled; an Inbox view; an OS notice when a turn ends in a background app. | `apps/desktop/src/app/chat/composer/controls.tsx:71,100,179-195`, `apps/desktop/src/lib/keybinds/actions.ts:406-413`, `apps/desktop/src/store/composer-status.ts:21-40`, `apps/desktop/src/store/session-dot-state.ts:84-98`, `apps/desktop/src/store/native-notifications.ts:153-160` |
| nanobot | Stop replaces Send; `/stop`; a guidance message may be queued; sidebar rows show a spinner, an orange dot for "updated", a warning for "needs attention"; a goal strip and a sub-agent dock; a browser notice when the turn ends, awaits the person or fails, only when the tab is hidden. | `webui/src/components/thread/ThreadComposer.tsx:1475-1478,1608-1614,1932-1935`, `webui/src/components/ChatList.tsx:1959-2008`, `webui/src/lib/browser-notifications.ts:77-81,106` |
| pi | The footer says "Working"; Esc interrupts; Enter steers; Alt+Enter queues; Alt+Up brings queued messages back; aborting returns queued messages to the editor. | `packages/coding-agent/docs/usage.md:35-40`, `packages/coding-agent/src/core/keybindings.ts:26,67-74` |
| omp | A "Working…" row; Esc interrupts; the agent hub lists sub-agents with status and unread counts (`x` aborts one); a "running subagents" badge; a terminal notice on completion and on error, on by default. | `packages/tui/src/app-keybindings.ts:97,141,172,186`, `packages/tui/src/overlays/agent-hub.ts:1-12`, `packages/tui/src/overlays/running-subagent-badge.ts:15-19`, `packages/coding-agent/src/modes/settings.ts:1417-1428` |
| `CopilotKit/OpenBot` | Send turns into "Stop the Bot"; a typing badge on the avatar; one sidebar badge counts everything waiting on the person; the roster opens with a "Needs you" card; a browser notice when a bot starts needing the person. | `app/src/components/channels/composer/composer.tsx:184-185,1533,1546-1556`, `app/src/components/bot-profile/attention.tsx:51-56,191-232,293`, `app/src/components/bots/bot-roster.tsx:37-47` |
| `nightly-labs/openbot` | Stop replaces Send when the agent works; the sidebar badge ranks states: waiting (question, approval, takeover) above working above unread above responded; the avatar face follows the same state; a queue panel with a steer action per row; per-agent setting for what a message sent while busy does. | `packages/ui/src/features/conversation/ComposerSendControl.tsx:1-8,57-65`, `packages/ui/src/features/sidebar/sidebar-agent-states.ts:81-99,164-169`, `packages/ui/src/features/conversation/QueuePanel.tsx:508-539` |
| Pattern seen in closed desktop apps | A mark in the sidebar for bots that need the person. A message shows at once, and a failed send returns to the box. A message can be queued while the bot is busy. | — |
| HIG, Nielsen | Show status "near the items it describes"; keep people informed "within a reasonable amount of time". | [Feedback](https://developer.apple.com/design/human-interface-guidelines/feedback), [heuristic 1](https://www.nngroup.com/articles/ten-usability-heuristics/) |

**Most share.** Stop replaces Send while busy; a queue or steer choice while running; the bot list shows busy and needs-you.

**Differ.** The list's mark (arc, dot, badge, avatar mood). Where a notice goes (OS, browser, terminal). Whether a plan or steps panel exists (pi has none).

**Recommendation for Bate (R3, R5).** Give every row of the bot list one state, ranked as `nightly-labs/openbot` does: waiting for the person, working, unread, idle; add a count badge on the Dock icon. Send a macOS notification when a bot needs the person or finishes while the window is in the background (Hermes does it only then). Stop gets Esc and an Esc hint in its tooltip (OpenClaw); "Queue" and "Steer" get one-line labels shown, not only tooltips.

---

## 7. Approving a risky action

**Bate today.** A card between the list and the composer, with the service's own button words; only in the open chat; the bot looks "working" in the list (B07, B25). Evidence: `Chat.tsx:379-389`, `main.tsx:188`.

| Who | What a person sees | Evidence |
|---|---|---|
| OpenClaw | A card above the composer shows the exact command with a `$`, the folder and a details popover (host, resolved path, scope). Allow once, Always allow, Deny. Shortcuts are Ctrl/Cmd chords only. "N more pending" opens a queue; the sidebar's attention panel can also approve. Modes per session: Read only, Ask me, Auto-review, Full access. | `ui/src/pages/chat/chat-view.tsx:155-176`, `ui/src/components/exec-approval-content.tsx:130-162,180-195`, `ui/src/i18n/locales/en-approvals.ts:22-32`, `ui/src/components/sidebar-attention.tsx:272-281` |
| Hermes | A stacked card in the transcript shows the command: Run, Reject, and a menu with "Allow this session" and "Always allow…" (which opens a dialog naming the permanent pattern). Enter runs, Esc rejects. An approval for another session fires an OS notice that names the session, even while the app has focus. Modes (Manual, Smart, Off) in a status-bar menu. | `apps/desktop/src/components/assistant-ui/tool/approval.tsx:45,205-217,285-295,352-394`, `apps/desktop/src/lib/keybinds/approval-keys.ts:35,79`, `apps/desktop/src/store/native-notifications.ts:40,153-155`, `apps/desktop/src/app/shell/approval-mode-menu.tsx:32,65,79` |
| nanobot | None found for tool calls (searched `approv`, `permission`, `confirm` in `webui/src/components`). Only channel pairing is approved or denied. | `webui/src/components/pairing/ChannelPairing.tsx:112`, `webui/src/components/pairing/PairingCodePopup.tsx:176` |
| pi | No built-in prompt. An example extension blocks a command with a select dialog showing the exact command (Yes/No). Policy belongs to the extension. | `packages/coding-agent/examples/extensions/permission-gate.ts:15-30`, `packages/coding-agent/docs/extensions.md:84,987` |
| omp | A dialog replaces the composer: title, a Command block, Approve, Approve Once, per-file or per-folder grants (session or always), Deny; Ctrl+Enter approves, Esc denies; typing a message denies and steers; it can time out to deny. Modes `always-ask`, `always-ask-cached`, `write`, `yolo` (default `yolo`) in Settings or a flag. | `packages/tui/src/overlays/approval-dialog.ts:1-18`, `packages/coding-agent/src/modes/controllers/extension-ui-controller.ts:726-740,758-787`, `packages/coding-agent/src/tools/approval.ts:44,110,927-945`, `packages/coding-agent/src/tools/settings.ts:313-343` |
| `CopilotKit/OpenBot` | An inline card titled by the action: Allow once, Allow for this conversation, Always allow here, Deny (or "I did it myself"), each with a one-line explanation. One inbox backs the card, an Approvals page, a sidebar badge, the roster's "Needs you" card and a browser notice. Settings: "Extra vigilant", "Auto-review", rules per bot. | `app/src/components/approvals/inline-approval.tsx:29-60,62-135,185-199`, `app/src/components/approvals/ask-first-copy.ts:15-34`, `app/src/components/approvals/settings.tsx:121-175`, `app/src/components/bots/bot-roster.tsx:43-46` |
| `nightly-labs/openbot` | A card shows the folder and the exact command (or file scope, or permission list): Deny, Always allow (opens a confirm dialog), Allow; when the text was cut only Deny shows. An overlay shows a pending approval from any agent with approve and decline. Levels per agent: Workspace only, Ask first, Full access. | `packages/ui/src/features/conversation/ConversationPrompts.tsx:44-47,85-160`, `packages/ui/src/components/StandingApprovalConfirmation.tsx:4-28`, `packages/ui/src/features/dynamic-island/OpenBotDynamicIsland.tsx:1286-1345`, `packages/ui/src/features/conversation/AgentSettingsPanel.tsx:1490-1514` |
| Pattern seen in closed desktop apps | Approval cards in the conversation, with the command or file shown and the choices allow once, allow always or deny. A settings screen sets how often a bot asks. | — |
| HIG | Warn "when their action might destroy data"; "avoid displaying alerts for common, undoable actions". | [Alerts](https://developer.apple.com/design/human-interface-guidelines/alerts) |

**Most share.** The exact command, with its folder, is shown. The choices are once, a wider "always", and deny. "Always" gets a second confirm. Four of seven apps (OpenClaw, Hermes, `CopilotKit/OpenBot`, `nightly-labs/openbot`) show the card inline in the chat; omp uses a dialog in the composer's place; pi leaves it to an extension; nanobot has none.

**Differ.** Where a pending approval from another chat shows: an OS notice (Hermes), a sidebar panel (OpenClaw), a badge and inbox (`CopilotKit/OpenBot`), an overlay (`nightly-labs/openbot`), nowhere (pi, nanobot, omp). The scope of "always": this session, this conversation, a pattern, a folder.

**Recommendation for Bate (R4, R3).** Keep the card in the chat; make it show the command, the folder and what "Always" covers (the service already has levels and grants: `Settings.tsx:10-18,100-112`). Three buttons with weight: Allow once, Always (asks again), Deny. A destructive default must not be the first focused button. Esc denies, as in Hermes and omp; approving by key needs a chord (OpenClaw). One count of everything waiting, in the bot list, the Dock badge and a notification (R3). Keep a typed message as "deny and steer" (omp).

---

## 8. Finding an old conversation

**Bate today.** One endless chat per bot; ⌘F in a bot and ⌘K across bots; hits have no date; "Fork" stays in the same chat (B26, B27). Evidence: `Chat.tsx:10,79-95,263-268`, `Palette.tsx:33,47`.

| Who | What a person sees | Evidence |
|---|---|---|
| OpenClaw | A Pinned section; a Sessions page (a table that groups by category, agent or date, with an archived filter); row menu: pin, rename, fork, move, archive, delete; titles made later; search over titles and message text, also in the palette (⌘K or ⌘P). | `ui/src/lib/sessions/grouping.ts:11-18`, `ui/src/pages/sessions/sessions-page-menu.ts:144,176,221,240,259`, `ui/src/components/command-palette-session-search.ts:1-60` |
| nanobot | Sidebar groups: Pinned, Today, Yesterday, Earlier, or by project. Row actions: pin, rename, archive, delete. The server writes the title after the turn. ⌘K opens "Search loaded content": titles, previews and only messages already loaded. | `webui/src/lib/chat-groups.ts:17-27,46-49`, `webui/src/components/ChatList.tsx:232-238`, `webui/src/components/SessionSearchDialog.tsx:65-80`, `nanobot/session/webui_turns.py:154` |
| Hermes | Recents with date dividers (or none, or by status), Pinned, projects. Row menu: rename, pin, colour, branch, export, move to project, archive or restore, delete with a confirm. Titles arrive live. An inline field does full-text search over all sessions, debounced. A keyboard session switcher. | `apps/desktop/src/app/chat/sidebar/sessions-section.tsx:189`, `apps/desktop/src/app/chat/sidebar/session-actions-menu.tsx:371-390,520-545`, `apps/desktop/src/app/session/hooks/use-message-stream/gateway-event/session-info.ts:560`, `apps/desktop/src/app/chat/sidebar/use-server-session-search.ts:14-19` |
| pi | No sidebar. `/resume` opens a picker with a current-folder or all scope, sorts (threaded, recent, fuzzy) and a "named" filter; Ctrl+R renames, Ctrl+D deletes after a confirm; an unnamed row shows the first message; search is fuzzy on name and folder, with `"phrase"` and `re:` over message text. | `packages/coding-agent/src/modes/interactive/components/session-selector.ts:23,121-137,148-170`, `packages/coding-agent/src/modes/interactive/components/session-selector-search.ts:26-32,88-93,120` |
| omp | `/resume` with "Current folder / All projects" tabs and groups Pinned, Today, Yesterday, This week; `/pin`, `/rename` (empty title: generate one), delete with a confirm; search over id, title, folder, first message and all text. | `packages/tui/src/overlays/session-selector.ts:144,152-157,283-290,296-313`, `packages/coding-agent/src/slash-commands/builtin-lifecycle.ts:216,421,457,678` |
| `CopilotKit/OpenBot` | A list of conversations, pinned first, then newest; right-click: Pin, Rename…, Delete…. Title is a server summary, else "untitled". Sidebar search is server-side over names and recent messages, 2 characters minimum, 250 ms debounce; the scope is stated. ⌘K opens a palette of pages, bots and conversations. | `app/src/components/app-sidebar/channel.tsx:100-160`, `app/src/components/app-sidebar/app-sidebar.tsx:121-126,142-152`, `app/src/components/channels/rename-conversation-dialog.tsx:64`, `app/src/components/navigation/command-menu.tsx:326` |
| `nightly-labs/openbot` | Conversations are threads inside a bot, shown as tabs and a menu (new, archived, restore). Bot menu: pin, mark read, mute, new thread, open window, edit, duplicate, hide, delete. ⌘K global search has tabs All, Agents, Channels, Messages, Files, Routines. | `packages/ui/src/features/sidebar/SidebarAgentContextMenu.tsx:45-131`, `packages/ui/src/features/conversation/threads/ThreadMenu.tsx:114-137`, `packages/ui/src/components/GlobalSearch.tsx:45` |
| Pattern seen in closed desktop apps | A bot has many conversations, listed on one page, with unread marks. A palette finds bots and conversations. A message can be forked. | — |

**Most share.** More than one conversation; pin and rename in most, delete in all, archive in four (OpenClaw, nanobot, Hermes, `nightly-labs/openbot`); a title made for the person; a ⌘K or a search field that finds sessions. All seven keep more than one conversation; Bate keeps one per bot.

**Differ.** Search depth: titles and loaded messages (nanobot), all text (OpenClaw, Hermes, pi, omp, `CopilotKit/OpenBot`). Whether the unit is a session list (most) or a thread inside a bot (`nightly-labs/openbot`).

**Recommendation for Bate (R8).** Give a bot many conversations: a "New chat" button, a list under the bot (or tabs, as `nightly-labs/openbot`), auto-titles that a rename does not freeze (`CopilotKit/OpenBot` `app/src/components/channels/rename-conversation-dialog.tsx:64`; OpenClaw derives titles in `ui/src/lib/session-display.ts:253`), pin and archive. Show a date and the bot on every search hit and state the scope in the box, as `CopilotKit/OpenBot` does. "Fork" opens a new chat "from here" (nanobot's `fork_chat`: `docs/websocket.md:233`). The owner chose many conversations (#149; ADR 0002).

---

## 9. Settings: app-wide, per bot, per project

**Bate today.** 11 sections in one scroll inside a bot; 6 are app-wide; no confirmation after Save; no ⌘, (B63–B73). Evidence: `Settings.tsx:71-181`, `main.tsx:133`.

| Who | What a person sees | Evidence |
|---|---|---|
| OpenClaw | One full-page settings area with a left nav of 6 groups (personal, device, connections, agents, security, system), about 30 routes, and a "Search settings" box. Per-agent configuration is on the Agents page, where an agent's tool profile overrides the global one. | `ui/src/app-navigation.ts:260-288`, `ui/src/components/settings-sidebar.tsx:361-373`, `ui/src/pages/agents/panels-tools-skills.tsx:105-114` |
| Hermes | A full-screen settings overlay: 9 sections (Model, Chat, Appearance, Workspace, Safety, Browser, Memory, Voice, Advanced) and about 11 more pages. A search pill opens the palette scoped to settings. An "Applies to" chip row picks which bot (profile) the page edits, and is hidden when there is one. | `apps/desktop/src/app/settings/constants.ts:712-848`, `apps/desktop/src/app/settings/index.tsx:268-272,424-450`, `apps/desktop/src/app/settings/profile-scope.tsx:44-50,76-102` |
| nanobot | A settings page with 8 nav items (Overview, Appearance, Notifications, Models, Capabilities, System, Advanced, About). Apps, Skills, Automations and Channels are separate sidebar buttons with their own pages. No settings search found; one bot, so no per-bot level. | `webui/src/components/settings/SettingsSidebar.tsx:32-41`, `webui/src/components/Sidebar.tsx:226-262` |
| pi | `/settings` is one flat list with search. Two layers: user settings and project settings; project settings are read only after the project is trusted. | `packages/coding-agent/src/modes/interactive/components/settings-selector.ts:161-586,999`, `packages/coding-agent/docs/settings.md:151,162`, `packages/coding-agent/src/modes/interactive/components/trust-selector.ts:54-68` |
| omp | `/settings` is a full-screen tab strip of 10 tabs; type-to-search with a match count. `/agents` is a per-agent hub. MCP and plugins take `--scope user` or `project`. | `packages/tui/src/overlays/settings-defs.ts:20-31`, `packages/tui/src/overlays/settings-selector.ts:631,712-731` |
| `CopilotKit/OpenBot` | Two levels, the closest to Bate: App Settings with about 10 nav items (General, Notifications, Vault, Organization, API keys, Computer, Approvals, Activity, Setup); each bot has its own page of 14 sub-routes (access, approvals, apps, memory, routines, skills, …) shown as rows with counts. No settings search found. | `app/src/components/settings/settings-sidebar.tsx:80-149`, `app/src/routeTree.gen.ts:108-121`, `app/src/components/bot-profile/settings-rows.tsx:102-205` |
| `nightly-labs/openbot` | A settings modal with 13 tabs in 3 groups (ungrouped, Account, This computer); per-bot edit and Skills open from the bot's menu; no search in the modal, but ⌘K has a "settings" section. | `src/renderer/src/features/settings/settings-tabs.ts:68-255`, `packages/ui/src/components/GlobalSearch.tsx:98,167`, `packages/ui/src/features/sidebar/SidebarAgentContextMenu.tsx:95` |
| Pattern seen in closed desktop apps | A settings overlay with a handful of sections, opened by ⌘,. A bot's own settings are a separate panel. | — |
| HIG | Minimise the number of settings; open with ⌘, from the App menu; keep task-specific options in the task; restore the last pane. | [Settings](https://developer.apple.com/design/human-interface-guidelines/settings) |

**Most share.** One app-wide settings screen with a left nav; bot-specific things live with the bot, or in a scope chip, not in the app screen. In the files read, sign-in is an app-level screen in all seven (the earlier map of provider screens: [`../components/08-window-remote-updates.md`](../components/08-window-remote-updates.md), evidence 7).

**Differ.** Count of sections (6 to 30), whether a search exists (OpenClaw, Hermes, pi, omp yes), and how the bot is chosen (chips in Hermes, a page per bot in `CopilotKit/OpenBot`, none in nanobot).

**Recommendation for Bate (R9).** Open an app Settings window with ⌘, : Sign-in and models, Devices, Chat apps, Helper permissions, Updates, Bug report. A bot's own settings (name, instructions, avatar, permissions, engine, delete) open from the bot; a project's from the project. Show rows with counts on the bot page, as `CopilotKit/OpenBot` does. Which scope owns which setting is ADR 0002 and #142; how the screens look is #138.

---

## 10. Installing add-ons (connectors, skills, workplaces)

**Bate today.** Connectors, Skills per bot; Workplaces under Projects; typed folder paths; a catalog behind a button; a preview and a "Nothing is downloaded until you agree" step for workplaces only (B36–B40, B44–B46, B58). Evidence: `Skills.tsx:25,55`, `Workplaces.tsx:104-140`, `Connectors.tsx:39-77`.

| Who | What a person sees | Evidence |
|---|---|---|
| OpenClaw | Settings has Plugins (a catalogue grid, detail, installed list), Skills (Installed, and Discover from a registry with search) and an MCP server form. One-click install, with a consent dialog that lists what it may do ("Your grants", prompt changes, conversation access) before a plugin installs. Skills show a registry scan verdict. A toggle per plugin. Per-agent policy is on the Agents page. | `ui/src/app-navigation.ts:268-275`, `ui/src/pages/plugins/consent-dialog.tsx:215-221,295-321`, `ui/src/pages/skills/discovery-view.tsx:105-120,216,256-260`, `ui/src/pages/config/mcp-server-editor.tsx:178-241` |
| Hermes | A Capabilities page with tabs Skills, Plugins, Connectors, Tools, search and a bot scope selector. One install dialog serves the catalogue, deep links and a pasted git URL; it marks each as Official, Verified or Unverified, pins the reviewed commit and offers "Install anyway" when review is not available. A connector form takes a command, or a URL with headers, with a paste-config box. | `apps/desktop/src/app/capabilities/index.tsx:171-185`, `apps/desktop/src/app/settings/plugin-install-modal.tsx:114-117,155-160,277-280`, `apps/desktop/src/app/capabilities/connectors/add-server-form.tsx:51-58,113-139` |
| nanobot | An Apps page (presets, a custom form, import) and a Skills page (Installed and Discover, provider filter, search). "Install {name}?" warns that third-party skills "may include instructions or executable scripts" and shows the source. MCP has test, reconnect, a per-tool allowlist and sign-in. Scope is global. | `webui/src/components/settings/SkillsMarketplace.tsx:169-195,317-353`, `webui/src/components/settings/system/AgentPluginDialog.tsx:34-47,57`, `webui/src/components/settings/system/McpManagementDialog.tsx:24,83-120` |
| pi | No catalogue UI. `pi install <source>` is a command; `pi config` is a terminal screen where Space toggles and Tab switches global or project scope. The only warning is in the docs: packages run code. | `packages/coding-agent/src/cli/args.ts:290-298`, `packages/coding-agent/src/modes/interactive/components/config-selector.ts:27,196-210`, `packages/coding-agent/docs/packages.md:21-23,78` |
| omp | Slash commands: `/marketplace` (add, discover, install with an interactive browser, upgrade, `--scope`), `/plugins`, `/skills`, `/mcp` (add, test, reauth), `/extensions` (a dashboard where Space toggles). No pre-install permission screen found. Errors appear as "Marketplace error: …" in the status line. | `packages/coding-agent/src/slash-commands/builtin-marketplace.ts:46-65,86,420,426-448`, `packages/coding-agent/src/slash-commands/builtin-skills.ts:66-76`, `packages/tui/src/overlays/extensions/extension-dashboard.ts:10-14,92` |
| `CopilotKit/OpenBot` | A Connections page (app tiles, plus two catalogues). "Add server" takes a name and an address and sends the person to that server's sign-in. Each bot's page has "Apps it can use" and "Skills it has", an allowlist. Runtime approval is the inline card (section 7). Errors inline with "Connect again". | `app/src/routes/_authed/_app/connections.new.tsx:4`, `app/src/components/connections/add-server-page.tsx:35-39,115,136-138,277-284`, `app/src/components/bot-profile/access.tsx:398-440` |
| `nightly-labs/openbot` | A Marketplace of Apps, Skills and Agents with detail pages. An app page has a Connection section and an "Ask macOS for access at install" switch; a per-app access chooser picks which bots may use it. Per-bot Skills modal: switch, install, update, remove, with an error line. | `packages/ui/src/features/marketplace/MarketplaceAppPage.tsx:190,485-508`, `packages/ui/src/features/marketplace/MarketplaceAppAccess.tsx:121,159-169,286-292`, `src/renderer/src/features/conversation/AgentSkillsModal.tsx:253-274,404-408` |
| Pattern seen in closed desktop apps | An add-on catalogue with a switch per bot. Custom servers are added by command or address. A skills panel. | — |

**Most share.** One list of what is installed, each with an on/off switch; MCP servers added by address or command, then a sign-in step; a catalogue with search in five of seven.

**Differ.** Whether a catalogue exists (pi and omp: commands only); whether the risk is shown before the install (OpenClaw, nanobot, Hermes: yes; omp, pi: no); whether the scope is per bot (Hermes, `CopilotKit/OpenBot`, `nightly-labs/openbot`, OpenClaw) or global (nanobot).

**Recommendation for Bate (R10, R12).** One "Add-ons" screen: Connectors, Skills and Workplaces as three tabs, each with Installed (switch per row, scope as a list of bots and projects), and Browse (the catalogue, with search; keep Bate's rule that reading it is on request, but say why next to the button). Reuse the workplace preview as the one install dialog: what it needs, sizes, hosts, "Nothing is downloaded until you agree" (`Workplaces.tsx:120-130`), and apply it to skills and connectors. Replace every typed path with a folder picker (`Skills.tsx:55`, `Workplaces.tsx:137`, `Projects.tsx:198`). Show trust as a label (Hermes: Official, Verified, Unverified).

---

## What was not checked

- None of the apps was run. Everything is from source, docs and strings; behaviour that depends on a running server (what triggers a push notice, what a theme looks like, how fast a list scrolls) is not known.
- The other six apps in [`../technology/README.md`](../technology/README.md) have no clone here and were not read.
- Closed apps appear as patterns only; none is cited line by line and none was re-run for this file.
- OpenClaw's native sidebar and project support in its macOS app; Hermes's group workspace route code; omp's browser guest client (`packages/collab-web`) for approvals; `CopilotKit/OpenBot`'s sign-in page; whether OpenClaw makes a first agent (only a test fixture shows it: `src/wizard/setup.default-agent.test.ts:192`).
- Counts of settings sections come from the apps' navigation lists; an item hidden by role or platform would not show there.
- HIG and the heuristics were read as text; no HIG example images were looked at.
