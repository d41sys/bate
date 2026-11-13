# Engine protocol (v1)

The service (`appd`) runs one engine program per bot and talks to it over the program's standard input and output. Every engine (Bate's own `agentcore`, an ACP adapter, a test fake) implements this one interface. Types and helpers are in the `proto` crate.

## Which program runs

`$APPD_ENGINE` if set, else `agentcore` next to the service's executable, else `agentcore` on `PATH` (`proto::engine_program`). A relative `APPD_ENGINE` path is resolved against the service's working directory, not the bot's folder. Set `APPD_ENGINE` to develop, test or swap an engine without changing the service.

For a bot that runs on an installed agent (`set_agent`), the same program is started with the arguments `acp <agent program> [arguments]`: see Installed agents.

The service starts the program with the bot's folder as its working directory (`proto::Engine::spawn`). The service copies each line of the engine's standard error into its log, tagged with the bot.

## Frames

- One JSON object per line, UTF-8, `\n`-terminated.
- Service → engine lines are capped at `proto::MAX_OP_FRAME` (1 MiB). Engine → service lines at `proto::MAX_EVENT_FRAME` (64 MiB), because tool output is forwarded whole.
- A line over the cap ends the connection (`read_frame` returns `FileTooLarge`; the rest of the line is unread). A line that is not a JSON object, or whose `type` is not in the lists below, is refused (`InvalidData`) and the next line is read normally.
- Field names are camelCase. `type` values are snake_case.

## Start-up

The engine's first line is `{"type":"ready","protocol":1}`. `proto::Engine::spawn` does not wait for it: the service reads it first and sends nothing before it. An engine whose `protocol` is not `proto::PROTOCOL` is stopped and not used.

## Operations (service → engine)

Each request is `{"id":"…","type":"<op>",…fields}`. `id` is a string chosen by the service. The engine answers each request exactly once with `{"type":"response","id":"…"}`, or with `"error":"text"` added when it fails. Match responses by `id`, never by order.

| `type` | Fields | Effect |
|---|---|---|
| `start` | `bot` | Begin a new conversation. The engine sends `session`. The engine works in its working directory, the bot's folder. |
| `open` | `bot`, `session` | Reopen an earlier conversation. The engine sends `session`. |
| `prompt` | `nonce`, `text` | Send a user message. `nonce` is the prompt ledger's key and comes back in `turn_start`. |
| `steer` | `text` | Add a message to the turn that is running. |
| `abort` | none | End the running turn now. It ends with `turn_end` reason `aborted`. |
| `answer` | `question`, `choice` | Answer an open `question`. `choice` is one of its `options`. |
| `set_model` | `model`, `effort?` | Model and thinking effort for the following model calls. |
| `rewind` | `entry`, `fork` | Go back to `entry`, the `nonce` of an earlier prompt: the conversation is as it was just before that prompt. In place, or as a new conversation when `fork` is true (the engine sends the new `session`; the old one is left as it was). |
| `compact` | none | Summarise the working context now. The response comes when the summary is in place. |
| `connectors` | `connectors` | The bot's MCP servers (`proto::connectors`), the whole list each time. The engine uses the enabled ones from its next turn on. |
| `peers` | `peers`, `leads` | The bots this one may message (`proto::groups::Peer`: `id`, `name`, `lead`), the whole list each time, and whether this bot leads a group. |
| `reply` | `message`, `text?`, `error?` | What came of the open `message`: the other bot's reply, or why there is none. |
| `browser_result` | `request`, `output?`, `error?` | What came of the open `browser` request: the page after the action, or why not. |
| `stop` | none | Finish and exit. The engine answers, then closes its output and exits with status 0. |

An engine also exits when its standard input closes: the service has stopped or crashed, and a new service starts new engines.

## Events (engine → service)

| `type` | Fields | Meaning |
|---|---|---|
| `ready` | `protocol` | First line; the protocol version the engine speaks. |
| `response` | `id`, `error?` | Outcome of a request. |
| `session` | `session` | The conversation now open. |
| `turn_start` | `nonce` | The prompt with this nonce started a turn. |
| `text` | `delta` | Reply text, appended to the current reply. |
| `thinking` | `delta` | Thinking text, appended to the current thinking. |
| `tool_start` | `call`, `tool`, `args` | A tool call began. `args` is a JSON object. |
| `tool_end` | `call`, `output`, `isError` | The call's result. |
| `question` | `question`, `title`, `options`, `kind`, `target` | The engine waits until the service sends `answer`. With `kind` (`read`, `write`, `run`, `connector`) and `target` (the exact path, command or connector tool) it asks whether that action may run: the service answers from the bot's permissions and asks the user only where they say so. What is asked about is what then runs. |
| `message` | `message`, `to`, `text` | A message for the bot `to`. The engine waits until the service sends `reply` for `message`. Not part of the transcript. |
| `browser` | `request`, `action` | An action in the bot's browser (see Browser). The engine waits until the service sends `browser_result` for `request`. Not part of the transcript. |
| `turn_end` | `reason`, `error?` | The turn ended: `done`, `aborted` or `error`. |

A killed engine sends nothing more.

## `agentcore` (Bate's own engine)

- **Model:** one provider, the OpenAI-compatible streaming chat API (`POST <base>/chat/completions`, `stream: true`). `BATE_MODEL_BASE_URL` (for example `http://127.0.0.1:1234/v1` for LM Studio) and, if needed, `BATE_MODEL_API_KEY` come from the environment until #8. `appd launch` passes `BATE_MODEL_BASE_URL`, `BATE_MODEL` and `BATE_MODEL_CONTEXT` on to the service; the key is not written into the launchd plist. The model is `set_model`'s, else `BATE_MODEL`. `delta.content` becomes `text`; `delta.reasoning_content` or `delta.reasoning` becomes `thinking`.
- **Tools:** `read` (numbered lines, `offset`, `limit`), `write`, `edit` (`old` must appear exactly once), `run` (`/bin/sh -c` in the bot's folder, `timeoutSecs` 120 by default and at most 600, output capped at 64 KiB), `search` (`grep -rnEI`). Relative paths resolve against the bot's folder.
- **Approval:** every tool call asks first: a `question` with an id of its own, options `Approve` and `Deny`, with `kind` and `target`: `read` and `search` are `read` (the path; `.` for a search with none), `write` and `edit` are `write` (the path), `run` is `run` (the command), a connector's tool is `connector` (`<connector>__<tool>`). The engine itself allows nothing. A question takes one answer; a second is refused. `answer` with another question or choice is refused. A denied call returns "The user denied this action." to the model.
- **Sub-agents:** the tool `agent {task}` starts a sub-agent: the same model, tools and folder, and a conversation of its own that begins with `task` and is not kept. Its last message is the tool's result. Its tool calls are asked about and shown like any other (their `call` ids begin `agent<n>/`); what it says in between is not sent. A sub-agent may start another, 5 levels deep at most: at level 5 the tool is not offered and a call to it is refused. A sub-agent's conversation is not compacted and cannot be steered.
- **Other bots:** when `peers` is not empty the tool `message_bot {bot, text}` is offered and the system prompt names the peers (and says so when the bot leads a group). A call first asks a `question` of kind `connector` with target `bot:<id>`, then sends `message` and waits for `reply`; an abort ends the wait, not the other bot's work.
- **Turns:** `prompt` while a turn runs is refused (use `steer`). `abort` kills the running command, gives up a model call or connector call that is under way (within about 0.1 s, also when the server is sending nothing) and ends the turn `aborted`; calls not yet run get "Not run" results. `stop` during a turn aborts it and waits up to 5 s for its `turn_end` before answering.
- **Conversations:** `.agentcore/sessions/<session>.jsonl` in the bot's folder, one line per chat message or marker (where a prompt starts, where a compaction happened), written before it is used. On `open`, a tool call with no result (the engine was killed mid-call) gets the result "Interrupted: …", and the engine sends `tool_end` with `isError` for it after `session`.
- **Compaction:** the engine asks the model for a summary of all but the 4 most recent messages and shows the model the summary in their place. It does this on `compact`, and by itself before a model call when the context is over four fifths of `BATE_MODEL_CONTEXT` (tokens, 32000 if unset; size is estimated at 4 bytes per token). The summary is not streamed as a reply. If the model gives no summary, an automatic compaction is skipped and the turn goes on; `compact` answers with an error, also when it was aborted (`aborted`). The conversation file keeps every message.
- **Rewind:** `entry` is a prompt's `nonce`. In place, the file loses that prompt and what followed. With `fork`, the part before it is copied to a new conversation and the engine continues there. Refused while a turn runs.
- **Connectors (MCP):** on each turn the engine starts the enabled `stdio` servers it does not have yet (in the bot's folder, with the connector's `env` added) and reaches `http` ones by POST (JSON or an event stream back, `Mcp-Session-Id` kept). New servers are started side by side; a `stdio` server whose process has stopped is started again on the next turn. `abort` ends a start or a call that is under way. A tool appears to the model as `<connector>__<tool>`. Every connector tool call asks first, with the title `Use <connector>: <tool>`. A connector that fails to load is reported on standard error (the service log) and left out until its settings change or the engine restarts. A tool call waits at most 120 s.

## Window ⇄ service (v1)

`appd` is the per-user service (launchd label `com.oursnever.bate.appd`). Its data lives in `~/Library/Application Support/Bate/`, or in `$APPD_HOME`; a custom home gets the label `com.oursnever.bate.appd.<hash>` so a test never touches the real agent. `appd launch` registers and starts it; `appd unregister` stops and removes it. A second `appd` for the same home exits at once (`run/appd.lock`). The service log is `logs/appd.log`, capped at 5 MiB; older lines move to `appd.1.log` … `appd.3.log` and the oldest file is dropped.

### Transport

- Unix socket `run/appd.sock` in the data folder. The folder is mode 0700, the socket 0600, and the service refuses a peer running as another user.
- One JSON object per line. Window → service lines are capped at `proto::MAX_WINDOW_FRAME` (1 MiB); a longer line closes the connection.
- On connect the service sends `{"type":"hello","app":"appd","protocols":[1],"pid":N}`. The window answers with the request `hello {protocol: 1}`. Any other request before it gets `hello_required`.

### Requests and responses

Each request is `{"id":…,"type":"<command>",…fields}`. The reply is

```json
{"id":…,"type":"response","command":"<command>","success":true,"data":{…}}
{"id":…,"type":"response","command":"<command>","success":false,"error":"text","code":"<code>"}
```

Match replies by `id`, never by order. Codes: `hello_required`, `parse`, `unknown_command`, `bad_request`, `no_such_bot`, `no_such_question`, `no_such_routine`, `invalid_routine`, `no_such_connector`, `invalid_connector`, `sign_in_failed`, `no_such_grant`, `no_such_note`, `no_such_skill`, `invalid_skill`, `no_such_group`, `invalid_group`, `busy`, `export_failed`, `engine_failed`, `engine_error`, `engine_stopped`, `internal`.

| Command | Fields | `data` |
|---|---|---|
| `hello` | `protocol` | `{protocol}` |
| `list_bots` | none | `{bots:[Bot]}` |
| `create_bot` | `name` (1 to 80 characters) | `{bot: Bot}` |
| `open_bot` | `bot`, `limit?`, `before?` | `{bot: Bot, history:[Entry], more, questions:[question event]}`: the last `limit` entries (all without it) with `seq` below `before`, oldest first; `more` is `true` when there are older ones |
| `prompt` | `bot`, `nonce`, `text` | `{}` once the engine admitted it; `{duplicate:true}` or `{duplicate:true, uncertain:true}` for a known nonce (see Prompt ledger) |
| `answer` | `bot`, `question`, `choice` | `{}` once the engine took it; `no_such_question` if that question is not open. `Always` on a question about an action: see Permissions. |
| `steer` | `bot`, `text` | `{}` |
| `abort`, `compact` | `bot` | `{}` |
| `set_model` | `bot`, `model`, `effort?` | `{}` |
| `rewind` | `bot`, `entry` (a prompt's `nonce`), `fork` | `{}` once the engine went back. The service then removes that prompt and what followed from the bot's history and sends `history_changed`. With `fork` the bot continues on a copy and the engine keeps the old conversation file. The removed prompts' nonces stay used in the ledger, so a late retry of one is answered `duplicate` and does not undo the rewind: send new prompts with new nonces. |
| `list_routines` | `bot` | `{routines:[Routine]}` |
| `set_routine` | `bot`, `routine` | `{routine}`. No `routine.id`: create; with one: replace |
| `delete_routine` | `bot`, `routineId` | `{}`. Its past runs go with it. |
| `list_routine_runs` | `bot`, `routineId` | `{runs:[RoutineRun]}`, newest first, at most the last 50 |
| `list_connectors` | `bot` | `{connectors:[Connector]}` |
| `set_connector` | `bot`, `connector` | `{connector}`. Creates or replaces by `name` |
| `delete_connector` | `bot`, `name` | `{}` |
| `sign_in_connector` | `bot`, `name` | `{url}`: open it in the browser. `sign_in_failed` when the server cannot be reached or does not say how to sign in |
| `sign_out_connector` | `bot`, `name` | `{}`; the service forgets the tokens |
| `list_groups` | none | `{groups:[Group]}` |
| `set_group` | `group` | `{group}`; creates when `group.id` is missing. `invalid_group`, `no_such_group` |
| `delete_group` | `groupId` | `{}`, or `no_such_group` |
| `list_skills` | `bot` | `{skills:[{name, description}]}`, by name |
| `install_skill` | `bot`, `path` | `{skill}`; copies the skill folder at `path`, or `invalid_skill` |
| `delete_skill` | `bot`, `name` | `{}`, or `no_such_skill` |
| `list_memory` | `bot` | `{notes:[{name, bytes, modifiedMs}]}`, by name |
| `read_memory` | `bot`, `name` | `{text}`, or `no_such_note` |
| `write_memory` | `bot`, `name`, `text` | `{}`; creates or replaces the note |
| `delete_memory` | `bot`, `name` | `{}`, or `no_such_note` |
| `set_disk_access` | `bot`, `wholeDisk` | `{}`; see Folder limit |
| `hand_back_browser` | `bot` | `{}`; the user gives the bot's browser back (see Browser) |
| `set_agent` | `bot`, `command` | `{}`; `command` is a list (program, then arguments) or `null`; see Installed agents |
| `get_permissions` | `bot` | `{levels: {read, write, run, connector}, grants:[Grant]}` |
| `set_permission` | `bot`, `kind`, `level` | `{}` |
| `delete_grant` | `bot`, `kind`, `target` | `{}`, or `no_such_grant` |
| `attach` | `bot`, `data` (base64), `name` (first part) or `upload` (later parts), `last?` | `{upload}` while `last` is not true, then `{path}`: the file, at most 25 MiB, is in `attachments/` in the bot's folder under `name`, or `1-name`, `2-name`… when taken. Parts go with the window that sent them; at most 4 files at once per window. Nothing is written through a link in the bot's folder. |
| `export_chat` | `bot` | `{path}`: the whole chat as Markdown in `exports/` under the service's home |
| `export_report` | none | `{path}`: a new bug-report archive (see below) |

`Bot` is `{id, name, createdAt, running, busy, wholeDisk, agent, browserTaken}` (`agent` only when set; `browserTaken` only when true): `running` while the bot has an engine, `busy` between `turn_start` and `turn_end`, and while a `compact` is unanswered. `bots_changed` is sent each time it changes. Every engine operation starts the bot's engine if it has none; the service then sends `start`, or `open` with the bot's last `session`. If the engine refuses `open`, the service forgets that session and sends `start`.

### Events (service → every window that said hello)

| Frame | Meaning |
|---|---|
| `{"type":"transcript","bot","seq","at","entry"}` | An entry was appended to the bot's transcript. |
| `{"type":"bots_changed"}` | Re-run `list_bots`. |
| `{"type":"question_closed","bot","question"}` | The engine took an answer to the question, or the turn that asked it ended. |
| `{"type":"engine_stopped","bot"}` | The bot's engine exited. Its open questions are dropped. |
| `{"type":"history_changed","bot"}` | Entries were removed (`rewind`). Re-run `open_bot`. |
| `{"type":"routines_changed","bot"}` | Re-run `list_routines`. |
| `{"type":"connectors_changed","bot"}` | Re-run `list_connectors`. |
| `{"type":"permissions_changed","bot"}` | Re-run `get_permissions`. |
| `{"type":"groups_changed"}` | Re-run `list_groups`. |
| `{"type":"skills_changed","bot"}` | Re-run `list_skills`. |
| `{"type":"memory_changed","bot"}` | A window changed a note: re-run `list_memory`. What the bot writes itself is not announced. |

### Transcript

The service appends every engine event except `ready` and `response` to the bot's transcript, plus `{"type":"prompt","nonce","text"}` for each prompt the engine admitted. When an engine stops mid-turn, the service appends `turn_end` with reason `error`, so history never shows a reply still running. An `Entry` is `{seq, at, entry}`; `seq` grows and `at` is Unix milliseconds. Windows read history from here (`open_bot`), so a window that was closed or disconnected mid-reply sees the whole reply. Engines belong to the service, so closing a window never stops a reply.

### Prompt ledger

Every prompt has a `nonce`. The service writes the nonce to the ledger (`sending`, committed to disk) before the prompt reaches the engine, and marks it `sent` when the engine answers the request. A nonce reaches the engine at most once:

| A `prompt` whose nonce is… | Reply |
|---|---|
| new | sent; `{}` once the engine admits it |
| `sending` or `sent` | `{duplicate:true}`; nothing is sent |
| uncertain: the engine or the service stopped while it was `sending` | `{duplicate:true, uncertain:true}`; nothing is sent. The window should tell the user and let them send it again under a new nonce |

A nonce the engine refused (`engine_error`), or that could not be sent because the engine did not start (`engine_failed`), is forgotten, so it may be sent again.

### Bots together

Same shapes as `proto::groups`. A group is `{id, name, lead, members}`: `name` is 1 to 80 characters, `lead` a bot's id, `members` 1 to 20 other bots; there are at most 50 groups, and a bot may be in several. The bots of a group may message each other; no other bots can. The service tells each running engine its bot's peers (`peers`) when it starts and after every change to the groups.

A message (`message` from an engine) is passed on by the service as a prompt to the other bot, with nonce `bot-<from>-<random>` and a first line that says which bot it is from and that it is not the user. What that bot says after its last tool call is the reply, sent back (`reply`) when its turn ends. No reply, and the reason instead, when: the two share no group; the other bot is in a turn or has an unanswered prompt (nothing is queued, and nothing interrupts it); its turn is stopped by its user or fails; its engine stops; or it says nothing. A reply is cut at 100 000 bytes.

A lead differs from a member in what its engine tells the model: that it leads, and to hand out tasks and put the results together. It has no way to stop or redirect another bot; only the user stops a bot. Each bot acts with its own permissions and its own folder limit, whoever messaged it, and its questions go to the user as always.

Whether a bot may message another is asked like a connector's tool: a `question` of kind `connector`, target `bot:<id>`. `Always` remembers it for that bot; the `connector` level of the sender applies.

### Skills

Same shapes as `proto::skills`. A skill is a folder with a `SKILL.md` that begins with a front matter:

```
---
name: pdf-forms
description: Fill in PDF forms. Use for any .pdf with fields.
---
```

`name` is 1 to 64 characters of a-z, 0-9 and `-`; `description` is one line of 1 to 1024 characters. Other keys are ignored. The rest of the file, and any other files in the folder, are the instructions and what they need.

`install_skill` copies the folder at `path` into `skills/<name>/` in the bot's folder, in place of a skill of the same name. It is refused (`invalid_skill`) when `path` is not a folder, has no valid `SKILL.md`, holds a link or a special file, or is larger than 500 files and folders, 20 MiB or 12 folders deep. `description` may hold no control or line-break characters. The copy is made in the service's own `staging/` folder and moved in whole.

The engine tells the model, at the start of every turn, each skill's name, description and file (`skills/<name>/SKILL.md`), at most 50, and to read that file before a task the skill is for. The instructions themselves are read with the ordinary `read` tool, so the bot's `read` permission applies. A folder whose name is not its skill's `name` is not a skill.

As with memory, the service never follows a link in the bot's folder: it holds the folder open and works on plain names in it.

### Memory

A bot's memory is the Markdown files in `memory/` in its folder: plain files the user can open in any editor. The engine puts them before the model at the start of every turn, in every conversation, in name order and up to 16 KiB; a note that does not fit, or is not text, is only named. The bot adds and changes notes with its ordinary `write` and `edit` tools, so the bot's `write` permission applies.

The memory commands are for the window's panel. A `name` is a file name ending in `.md`, at most 100 bytes, without `/` and not starting with a dot (`bad_request` otherwise); `text` is at most 64 KiB. The bot can write in its folder, so the service trusts nothing there: only plain files directly in a plain `memory` folder are listed, read or replaced, never what a link points to (a file with a second name elsewhere counts as one), and the engine shows the model exactly the notes `list_memory` lists.

### Folder limit

A bot is kept inside its folder (`bots/<id>` under the service's home) by the operating system unless `wholeDisk` is true; a new bot is limited. The service starts a limited bot's engine under macOS `sandbox-exec`, so the rule holds for the engine and for every program it starts, whatever was approved:

- **Writing:** only inside the folder.
- **Reading:** nothing under `/Users`, `/Volumes`, `/private/var/folders`, `/private/tmp`, `/cores`, Homebrew's `var`, the system's logs or the service's home, except the folder and the engine's program. System files stay readable.
- **Other programs:** no Apple events, no opening apps, no launchd jobs, no signals to or looks at processes outside (so no `pgrep`), no drivers, no shared memory, no local sockets outside the folder (so not the service's own socket, nor the user's key agent), no preferences (`defaults` would write into the user's home), and only the system services needed to look up names and check certificates (so not the keychain, the clipboard or the file index).
- **Environment:** `PATH`, `LANG`, `LC_ALL`, `USER`, `LOGNAME`, `SHELL`, `TERM` and the engine's own `BATE_` settings; nothing else of the service's. `HOME` is the folder and `TMPDIR` is `.tmp` in it, so programs keep their settings and temporary files there.
- **Network:** open, local ports included: the engine has to reach its model.

`set_disk_access` stores the choice and stops the bot's idle engine; the next thing the bot does starts an engine under the new rule. `bots_changed` follows. During a turn it is refused with `busy`. On a system without `sandbox-exec` a limited bot's engine does not start (`engine_failed`).

Not covered: what a bot sends over the network from its own folder; services listening on local ports; anything a bot started while it had the whole disk. A limited bot cannot use programs installed under `/Users` (a connector's command there cannot be read), and a program that insists on the system's own temporary folder (`mktemp` with no template, Xcode's tool cache) is refused there.

Whatever the limit, a command or connector the model starts does not get `BATE_MODEL_API_KEY`.

### Permissions

Same shapes as `proto::permissions`. For each bot and each kind of action (`read`, `write`, `run`, `connector`) there is a level: `always`, `ask` or `never`. A bot starts with `read` on `always` and the rest on `ask`.

The service decides every engine `question` that has a `kind`, on the `target` the engine is about to run:

- `always`: it answers `Approve` itself. `never`: it answers `Deny` itself, whatever was granted before. Neither is shown or stored as a question.
- `ask`: if a grant has exactly this `kind` and `target`, it answers `Approve` itself. Otherwise the question goes to the windows with the options `Approve`, `Always`, `Deny`.

`answer` with `Always` gives the engine `Approve` and, once the engine has taken it, stores a grant `{kind, target, atMs}` for that exact action. `Approve` is for this once. Grants are listed by `get_permissions` and removed by `delete_grant`; `permissions_changed` follows each change. `Always` on a question without a `kind` is refused (`bad_request`). A `connector` grant is for the tool, whatever its arguments, and goes when its connector is changed or removed (a connector's name has no `__`, so `<connector>__<tool>` names one tool). A target is compared as written: `a.txt` and `./a.txt` are two actions.

### Questions

An engine `question` is stored until the engine takes an `answer` to it (an answer the engine refuses leaves it open), its turn ends, or the engine stops. `open_bot` lists the open ones, so a window that reconnects still sees them.

### Routines

Same shapes as `proto::routines`: a routine is a prompt sent on a daily, weekly, monthly or yearly schedule at a wall time in an IANA zone, every `interval` periods counted from `anchor`. Two more frequencies count real time, starting at `time` on the anchor date: `hourly` (every 1 to 99 hours) and `minutes` (every 5 to 1440 minutes); a clock change neither skips nor repeats their runs. The service sets `id`, `anchor`, `nextRunMs` and `lastRun`. `name` is 1 to 80 characters, `prompt` 1 to 8000, and a bot has at most 50 routines.

At a slot the service sends `"[Routine: <name>]\n<prompt>"` with nonce `routine-<id>-<slotMs>`, through the ledger, so a crash cannot send it twice. A slot missed while the service was down runs once if it is at most 6 h old; an older one is recorded as `missed`. Every handled slot is kept as a `RoutineRun` (`atMs`, `outcome` `sent`, `failed` or `missed`, `error`); `routines_changed` is sent when one is added. A wall time that does not exist (clocks jump forward) runs when the gap ends; one that happens twice runs at the earlier instant.

A routine may leave out `schedule` when `webhook` is true, and may have both; one with neither is refused (`invalid_routine`). `enabled: false` turns off both.

#### Webhooks

The service listens on `127.0.0.1` only, port 47821 unless `APPD_HOOK_PORT` names another (`0`: any free one). If the port cannot be opened, the service logs it, runs without webhooks, and makes a new token for every routine that had one: whatever holds the port instead is being sent the old ones. A whole request must arrive within 5 s; 16 are served at once.

For a routine with `webhook: true` the service makes `hookToken` (32 hex characters, kept until `webhook` is turned off) and reports `hookUrl`, `http://127.0.0.1:<port>/hooks/<token>`. The token is the only secret: whoever on this Mac knows the URL can start the routine.

`POST <hookUrl>` with an optional body of at most 64 KiB (`content-length` required when there is one; chunked bodies get 411) sends `"[Routine: <name>]\n<prompt>"` with nonce `hook-<id>-<random>`. A body that is not blank follows the prompt, between two copies of a line `--- data <random> ---` made for that request, under a sentence that tells the model it is data from outside and not instructions. The body cannot know the line, so it cannot close its own section. The run is recorded like a scheduled one.

| Status | Meaning |
|---|---|
| 202 | Handed to the bot's engine. Whether the engine took it (it refuses while a turn runs) shows in the routine's runs. |
| 404 | No enabled routine has this token, or the path is not `/hooks/<token>`. |
| 409 | The bot is in a turn, or has a prompt its engine has not answered. Nothing is recorded; call again later. |
| 403 | The request has an `Origin` header: web pages may not call webhooks. |
| 405, 411, 413, 400 | Not `POST`; a chunked body; more than 64 KiB; a malformed request. |
| 503 | The bot's engine could not be started, or the service did not answer in 5 s. |

Not built yet: starting a routine from a message on a connected channel. There are no channels to receive one until Telegram or Discord is connected.

### Connectors

Stored per bot (`proto::connectors`): `name` is 1 to 40 characters of `a-z`, `0-9`, `_`, `-`; `transport` is `stdio` (`command`, `args`, `env`) or `http` (`url` starting with `http://` or `https://`, `headers`). Values are stored as plain text. The service sends a bot's list to its engine when the engine starts and after every change (`connectors`).

#### Signing in (OAuth)

A connector reached over http may set `oauth: true`: the server signs its users in. Its `url` must then be `https` (or `http` on this Mac) and its `headers` may not hold `authorization`. `list_connectors` adds `signedIn` to such a connector. `clientId` is this app's id at a server that does not register apps by itself.

The bot never holds the token. Its engine is sent the connector with `url` replaced by `http://127.0.0.1:<port>/mcp/<key>` (the service's local port, see Webhooks; `key` is 32 random hex characters, made per bot and connector), with no headers, and with `enabled` false until the user is signed in. Signing in or out sends the engine the list anew. The service's proxy there passes each `POST` on to the real `url` with the connector's own headers, `Authorization: Bearer <access token>`, and of the engine's headers only `content-type`, `accept`, `mcp-protocol-version`, `mcp-session-id` and `last-event-id`. The answer's status, `content-type`, `mcp-session-id` and body come back as they arrive (so a stream stays a stream). Redirects are not followed. A request body may be 1 MiB.

`sign_in_connector` finds the sign-in server (`/.well-known/oauth-protected-resource`, then `/.well-known/oauth-authorization-server` or `/.well-known/openid-configuration`; every address must be https; one on this Mac is taken only when the connector's own `url` is on this Mac; none may hold a user name), registers this app there unless `clientId` is set, and answers with the address to open: an authorization request with PKCE (`S256`), a random `state` and `resource` set to the connector's `url`. The browser comes back to `http://127.0.0.1:<port>/oauth/callback`; the service trades the code for tokens, stores them and sends `connectors_changed`. A `state` works once, for 10 minutes.

An access token within 30 s of its end is renewed before use; an answer of 401 from the server renews it once and sends the request again. Without tokens, or when renewing fails, the proxy answers 401 and the server is not asked. When the sign-in server refuses to renew, the tokens are forgotten: the connector shows as signed out. Tokens are for one address: changing a connector's `url`, `headers` or `oauth`, or removing it, forgets them. A connector that is off is not served.

Tokens are stored as plain text in `app.db`, like connector values. A bot limited to its folder cannot read that file.

### Browser

Each bot has a web browser of its own: the Chrome installed on this Mac (`/Applications/Google Chrome.app`, else Chromium; `$APPD_CHROME` names another), started by the service on the bot's first browser action, in its own window, with a profile of the bot's own (`browsers/<bot>` in the service's folder, so the bot's programs cannot read its cookies and its sign-ins are kept apart from the user's and other bots'). Without one, the action fails with a message saying so; Bate does not install a browser.

Bate's engine has a `browser` tool, asked about as kind `connector`, target `browser` (once for the browser, whatever the action). The action is sent as a `browser` event; the service checks it and drives the browser over the DevTools protocol. Only these exist, so nothing the bot runs can reach the browser another way:

| `action` | Fields | Does |
|---|---|---|
| `open` | `url` | Goes to the address: `http` or `https` only, never a file of this Mac or the browser's own pages. |
| `look` | none | Nothing. |
| `click` | `element` | Clicks the element with that number. |
| `type` | `element`, `text`, `submit?` | Puts `text` (at most 10 000 characters) in the field; with `submit`, sends its form. |
| `back` | none | Goes back. |

Every result is the page as it is after the action, once loaded (at most 15 s): `Title:`, `Address:`, the page's text (at most 20 000 characters) and `Elements:`, the numbered links, buttons and fields (at most 200) that `click` and `type` refer to; what is in a password field is not shown. A page that is not a web page (one the user opened) is not read. Downloads are refused: Chrome is not kept to the bot's folder.

The user watches in the browser's window. Clicking, tapping or typing in a page there takes the browser over (in the page's own frame; not in a frame from another site, the address bar or another tab): `browserTaken` becomes true (`bots_changed`), and the bot's next browser action waits, its turn with it, until `hand_back_browser`. An action that waited for a turn which has ended meanwhile (an abort) is not done. Scrolling does not take the browser over. An action already under way when the user takes over finishes.

The browser stays open while the service runs; a browser the user quit is started again by the next action.

### Installed agents (ACP)

A bot can run on an agent installed on this Mac that speaks the [Agent Client Protocol](https://agentclientprotocol.com) over its standard input and output, in place of Bate's own engine: `set_agent {bot, command: ["opencode", "acp"]}`. `null` goes back. The agent uses its own login, model and tools. A bare program name is looked up on the service's `PATH`.

- `set_agent` stores the choice, forgets the bot's open conversation (the other engine could not reopen it; the transcript stays), and stops the bot's idle engine. `bots_changed` follows. During a turn it is refused with `busy`.
- The agent's login is in the user's home, so such a bot runs only with `wholeDisk` true. Otherwise its engine does not start (`engine_failed`, saying so).
- `agentcore acp` starts the agent in the bot's folder and translates: `start` is `session/new`, `open` is `session/load` (refused by an agent that cannot), `prompt` is `session/prompt`, `abort` is `session/cancel`. The agent's message and thought chunks become `text` and `thinking`; its tool calls become `tool_start` (`tool` is the agent's kind of tool; `args` holds `title`, `rawInput`, `locations`) and `tool_end`.
- The agent's `session/request_permission` becomes a `question` with the options `Approve` and `Deny`. Its `kind` and `target` come from the agent's kind of tool (`read`, `search`: read; `edit`, `delete`, `move`: write; `execute`: run, with the command as target), so the bot's permission levels and grants apply; any other kind is asked of the user each time. The answer goes back as the agent's allow-once or reject-once option. What an agent does without asking, by its own settings, Bate does not see before it happens.
- The bot's enabled connectors are given to the agent when a conversation starts or is reopened (those over http only to an agent that takes them). A connector that signs in is given with the service's local address, as to Bate's own engine.
- Not offered to the agent: reading or writing files and running terminals through Bate (it does these itself). Refused by this engine: `steer`, `rewind`, `compact`. `set_model` is accepted and has no effect; choose the model in the agent. Memory, skills and messages between bots are Bate's own engine's and are not given to an agent.
- If the agent fails to open a conversation, the error names the agent's own ways to sign in.
- A turn that was aborted ends `aborted` however the agent ends it. A tool call the agent did not finish by the end of its turn gets a `tool_end` with `isError`. A tool's output is cut at 64 KiB.

### Bug report

`export_report` writes `reports/bate-report-<unixMs>.tar.gz` (mode 0600) under the service's home. It holds `logs/` (the service log and its older files) and `settings.json`: versions, and per bot its id, its routines (schedule, next and last run; no name, no prompt), and its connectors: the command, the number of arguments, the names of `env` and header entries, and a URL's scheme and host only. Conversations are not included.

Secrets are removed from the logs, and from `settings.json`, in three ways:

- Every stored connector value is replaced by `[removed]` where it is 4 characters or more: `env` values, header values and their parts, arguments with a `=` and their values, other arguments of 12 characters or more, a URL as a whole, its user and password, its query values and the pieces of its path of 12 characters or more. So is the service's `BATE_MODEL_API_KEY`.
- What follows one of these words and a `:` or `=` is removed, up to the next space or `&`, or up to the closing quote: `credential`, `signature`, `password`, `passwd`, `secret`, `token`, `pwd`, `sig`, and any word ending in `key` or `auth`. After `authorization`, `auth` and `cookie` the rest of the line goes. The word after `bearer`, and after a flag ending in one of the words (`--api-key value`), goes too.
- PEM blocks (`-----BEGIN` to the end of the `-----END` line) are removed.

Not recognised: a secret the service never stored that is printed with none of those words, and a stored value printed in another form (percent-encoded, base64).

### Storage

One SQLite database, `app.db`, in write-ahead mode with `synchronous=FULL`, written only by the service: bots, transcript, ledger, open questions, disk access, permission levels and grants, groups, sign-ins of connectors, routines, their past runs, and connectors. When the service starts, a ledger entry still `sending` becomes uncertain and open questions are dropped, because engines do not outlive the service.
