# Engine protocol (v1)

The service (`appd`) runs one engine program per bot and talks to it over the program's standard input and output. Every engine (Bate's own `agentcore`, an ACP adapter, a test fake) implements this one interface. Types and helpers are in the `proto` crate.

## Which program runs

`$APPD_ENGINE` if set, else `agentcore` next to the service's executable, else `agentcore` on `PATH` (`proto::engine_program`). A relative `APPD_ENGINE` path is resolved against the service's working directory, not the bot's folder. Set `APPD_ENGINE` to develop, test or swap an engine without changing the service.

For a bot that runs on an installed agent (`set_agent`), the same program is started with the arguments `acp <agent program> [arguments]`: see Installed agents.

The service starts the program with the bot's folder as its working directory (`proto::Engine::spawn`), and names the folder the bot works in in `BATE_WORK_DIR`: the folder of the bot's project, or the bot's folder again (see Projects). An engine keeps its own files (conversations, notes, skills) in its working directory, and runs its tools, and an installed agent, in `BATE_WORK_DIR`. The service copies each line of the engine's standard error into its log, tagged with the bot.

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
| `start` | `bot` | Begin a new conversation. The engine sends `session`. The engine works in `BATE_WORK_DIR`, without it in its working directory, the bot's folder. |
| `open` | `bot`, `session` | Reopen an earlier conversation. The engine sends `session`. |
| `prompt` | `nonce`, `text` | Send a user message. `nonce` is the prompt ledger's key and comes back in `turn_start`. |
| `steer` | `text` | Add a message to the turn that is running. The engine puts it into the conversation (and so into the saved session) before its next model call, and then sends `steered`. Without a turn it answers an error. |
| `abort` | none | End the running turn now. It ends with `turn_end` reason `aborted`. |
| `answer` | `question`, `choice` | Answer an open `question`. `choice` is one of its `options`. |
| `set_model` | `model`, `effort?` | Model and thinking effort for the following model calls; an empty `model` goes back to `BATE_MODEL`. |
| `rewind` | `entry`, `fork` | Go back to `entry`, the `nonce` of an earlier prompt: the conversation is as it was just before that prompt. In place, or as a new conversation when `fork` is true (the engine sends the new `session`; the old one is left as it was). |
| `compact` | none | Summarise the working context now. The response comes when the summary is in place. |
| `connectors` | `connectors` | The bot's MCP servers (`proto::connectors`), the whole list each time. The engine uses the enabled ones from its next turn on. |
| `instructions` | `text` | What the user told the bot to be and do, sent at start (when set) and on each change. The engine puts it first in its system prompt; empty: none. An installed agent (ACP) ignores it. |
| `step_limit` | `steps` | How many tool rounds a turn may take, 1 or more (`0` is refused); sent at start (when set) and on each change. Without it the engine allows 100. An installed agent (ACP) ignores it. |
| `skill_folders` | `folders` | Folders besides the bot's own whose `skills/` the model is told about (a workplace's, see Workplaces), the whole list each time; sent when the engine starts (when not empty) and when the list changes. The model sees them as the bot's own, named by their full paths. An installed agent (ACP) ignores it. |
| `peers` | `peers`, `leads` | The bots this one may message (`proto::projects::Peer`: `id`, `name`, `lead`), the other bots of its project, the whole list each time, and whether this bot leads its project. |
| `reply` | `message`, `text?`, `error?` | What came of the open `message`: the other bot's reply, or why there is none. |
| `browser_result` | `request`, `output?`, `error?` | What came of the open `browser` request: the page after the action, or why not. |
| `desktop_result` | `request`, `output?`, `error?` | What came of the open `desktop` request: what the helper saw or did, or why not. |
| `apple_result` | `request`, `output?`, `error?` | What came of the open `apple` request: what the app returned, or why not. |
| `token` | `request`, `url?`, `key?`, `api?`, `model?`, `context?`, `error?` | What came of the open `token` request: the endpoint and authorization token/key for the model call, or why there is none. `api` is `responses` when the endpoint speaks OpenAI's Responses API; without it, the chat API. `model` is the model to call when the engine has none from `set_model` or `BATE_MODEL`: on a ChatGPT sign-in the service sends the bot's model, and for a bot with none the first one the plan lists, which it keeps for the bot. `context` is that model's context size in tokens: on a ChatGPT sign-in, the `context_window` of the model's row in the plan's list, when the row has one; the service reads the list once for this, and again whenever a window asks for the models. With a model server and no model the service sends `error`. |
| `stop` | none | Finish and exit. The engine answers, then closes its output and exits with status 0. |

An engine also exits when its standard input closes: the service has stopped or crashed, and a new service starts new engines.

## Events (engine → service)

| `type` | Fields | Meaning |
|---|---|---|
| `ready` | `protocol` | First line; the protocol version the engine speaks. |
| `response` | `id`, `error?` | Outcome of a request. |
| `session` | `session` | The conversation now open. |
| `turn_start` | `nonce` | The prompt with this nonce started a turn. |
| `steered` | `text` | The running turn took a `steer` message into the conversation, at this point of the turn. |
| `text` | `delta` | Reply text, appended to the current reply. |
| `thinking` | `delta` | Thinking text, appended to the current thinking. |
| `tool_start` | `call`, `tool`, `args` | A tool call began. `args` is a JSON object. |
| `tool_end` | `call`, `output`, `isError` | The call's result. |
| `question` | `question`, `title`, `options`, `kind`, `target` | The engine waits until the service sends `answer`. With `kind` (`read`, `write`, `run`, `connector`, `apple_read`, `apple_change`) and `target` (the exact path, command or connector tool) it asks whether that action may run: the service answers from the bot's permissions and asks the user only where they say so. What is asked about is what then runs. |
| `message` | `message`, `to`, `text` | A message for the bot `to`. The engine waits until the service sends `reply` for `message`. Not part of the transcript. |
| `browser` | `request`, `action` | An action in the bot's browser (see Browser). The engine waits until the service sends `browser_result` for `request`. Not part of the transcript. |
| `desktop` | `request`, `action` | An action in other apps (see Desktop). The engine waits until the service sends `desktop_result` for `request`. Not part of the transcript. |
| `apple` | `request`, `tool`, `args` | An action in one of the user's Apple apps (see Apple apps). The engine waits until the service sends `apple_result` for `request`. Not part of the transcript. |
| `token` | `request` | The engine asks the service for the endpoint and token before a model call. The engine waits until the service sends `token` for `request`. Not part of the transcript. |
| `retry` | `attempt`, `of`, `waitMs`, `error` | A model call failed in a way that may pass; the engine asks again after `waitMs`. This is retry `attempt` of at most `of`; `error` is why. It is part of the transcript. |
| `turn_end` | `reason`, `error?` | The turn ended: `done`, `aborted`, `limit` or `error`. |

A killed engine sends nothing more.

## `agentcore` (Bate's own engine)

- **Model:** two providers. With `api: "responses"` in the `token` answer (a ChatGPT sign-in), the engine calls OpenAI's Responses API: `POST <base>/responses` with `store: false`, `stream: true`, the system prompt as `instructions`, the conversation as `input` items (a tool call as `function_call`, its result as `function_call_output`) and the tools as functions in one `namespace` named `bate`. `response.output_text.delta` becomes `text`; a `function_call` item that is done becomes a tool call; the reply is whole at `response.completed`, and `response.failed`, `response.incomplete` or `error` ends the turn with that reason. Thinking is not shown on this route. Otherwise the engine calls the OpenAI-compatible streaming chat API (`POST <base>/chat/completions`, `stream: true`). The service stores the model server base URL and optional API key (`set_provider`/`get_provider`), falling back to `BATE_MODEL_BASE_URL` and `BATE_MODEL_API_KEY`. Before each model call, the engine asks the service for the endpoint and token (`token` event and operation), so a token cannot expire mid-turn. `appd launch` passes `BATE_MODEL_BASE_URL`, `BATE_MODEL` and `BATE_MODEL_CONTEXT` on to the service; the key is not written into the launchd plist. The model is `set_model`'s, else `BATE_MODEL`. `delta.content` becomes `text`; `delta.reasoning_content` or `delta.reasoning` becomes `thinking`.
- **Tools:** `read` (numbered lines, `offset`, `limit`), `write`, `edit` (`old` must appear exactly once), `run` (`/bin/sh -c` in the working folder, `timeoutSecs` 120 by default and at most 600, output capped at 64 KiB), `search` (`grep -rnEI`). Relative paths resolve against the working folder: the project's for a bot in one, otherwise the bot's.
- **Approval:** every tool call asks first (the Apple app tools: see Apple apps; `plan` never asks): a `question` with an id of its own, options `Approve` and `Deny`, with `kind` and `target`: `read` and `search` are `read` (the path; `.` for a search with none), `write` and `edit` are `write` (the path), `run` is `run` (the command), a connector's tool is `connector` (`<connector>__<tool>`). The engine itself allows nothing. A question takes one answer; a second is refused. `answer` with another question or choice is refused. A denied call returns "The user denied this action." to the model.
- **Sub-agents:** the tool `agent {task}` starts a sub-agent: the same model, tools and folder, and a conversation of its own that begins with `task` and is not kept. Its last message is the tool's result. Its tool calls are asked about and shown like any other (their `call` ids begin `agent<n>/`); what it says in between is not sent. A sub-agent may start another, 5 levels deep at most: at level 5 the tool is not offered and a call to it is refused. A sub-agent's conversation is not compacted and cannot be steered.
- **Plan:** the tool `plan {steps}` takes the whole list of a task's steps, each `{step, status}` with status `pending`, `in_progress` or `done`, and answers "Plan updated.". A list that is empty, or has a step without text or with another status, is refused. It asks no `question` and the engine keeps nothing: the steps are the `args` of the call's `tool_start`. The window shows them as a checklist, the newest of a turn in place of the older ones (a sub-agent's apart from the conversation's).
- **Other bots:** when `peers` is not empty the tool `message_bot {bot, text}` is offered and the system prompt names the peers (and says so when the bot leads its project). A call first asks a `question` of kind `connector` with target `bot:<id>`, then sends `message` and waits for `reply`; an abort ends the wait, not the other bot's work.
- **Turns:** `prompt` while a turn runs is refused (the service queues it instead, see Queue and steer). A `steer` is taken in between model calls, and once more before a turn with nothing left to run would end. `abort` kills the running command, gives up a model call or connector call that is under way (within about 0.1 s, also when the server is sending nothing) and ends the turn `aborted`; calls not yet run get "Not run" results. `stop` during a turn aborts it and waits up to 5 s for its `turn_end` before answering.
- **Limits:** a model call that fails with a network error, an HTTP 429 or a 5xx, or that sends no data for 180 s (`BATE_MODEL_IDLE_MS` in milliseconds, for tests), is asked again up to 5 times, after 2 s, 4 s, 8 s, 16 s and 32 s (`BATE_MODEL_RETRY_MS` sets the first), or after the seconds a `Retry-After` header names (at most 120). Before each wait the engine sends `retry`; `abort` ends the wait within about 0.1 s. A 401, 403 or 400 (a bad key, a refused request), a 429 that is a quota (`insufficient_quota`, a ChatGPT `usage_limit_exceeded`) and a failure in the middle of a reply that already sent text are not asked again: the turn ends `error`. A turn stops before its next model call once it has run `step_limit` tool rounds (a model reply with tool calls, with all its calls run; 100 unless the service sent `step_limit`): `turn_end` reason `limit`, `error` "stopped after N tool rounds". The conversation then ends on tool results and nothing is lost: the window's Continue sends a `prompt` ("Continue.") and the model goes on, with a new count. A sub-agent has the same limit; its tool then fails with "The sub-agent stopped after N tool rounds.".
- **Conversations:** `.agentcore/sessions/<session>.jsonl` in the bot's folder, one line per chat message or marker (where a prompt starts, where a compaction happened), written before it is used. On `open`, a tool call with no result (the engine was killed mid-call) gets the result "Interrupted: …", and the engine sends `tool_end` with `isError` for it after `session`.
- **Compaction:** the engine asks the model for a summary of all but the 4 most recent messages and shows the model the summary in their place. It does this on `compact`, and by itself before a model call when the context is over four fifths of the model's size (size is estimated at 4 bytes per token). The model's size is `BATE_MODEL_CONTEXT` (tokens) when set, else the `context` of the `token` answer, else 32000; a model server (not a ChatGPT sign-in) is not asked for it. It also does it when the model server refuses a call because the conversation is more than the model holds (recognised by the server's words, such as `context_length_exceeded`): the call is then made once more with the summary; if nothing was old enough to summarise, or the summary fails, the turn ends with the server's refusal. The summary is not streamed as a reply. If the model gives no summary, an automatic compaction is skipped and the turn goes on; `compact` answers with an error, also when it was aborted (`aborted`). The conversation file keeps every message.
- **Rewind:** `entry` is a prompt's `nonce`. In place, the file loses that prompt and what followed. With `fork`, the part before it is copied to a new conversation and the engine continues there. Refused while a turn runs.
- **Connectors (MCP):** on each turn the engine starts the enabled `stdio` servers it does not have yet (in the bot's folder, with the connector's `env` added) and reaches `http` ones by POST (JSON or an event stream back, `Mcp-Session-Id` kept). New servers are started side by side; a `stdio` server whose process has stopped is started again on the next turn. `abort` ends a start or a call that is under way. A tool appears to the model as `<connector>__<tool>`. Every connector tool call asks first, with the title `Use <connector>: <tool>`. A connector that fails to load is reported on standard error (the service log) and left out until its settings change or the engine restarts. A tool call waits at most 120 s.

## Window ⇄ service (v1)

`appd` is the per-user service (launchd label `com.oursnever.bate.appd`). Its data lives in `~/Library/Application Support/Bate/`, or in `$APPD_HOME`; a custom home gets the label `com.oursnever.bate.appd.<hash>` so a test never touches the real agent. `appd launch` registers and starts it; `appd unregister` stops and removes it. From `Bate.app` (an `appd` in the app's `Contents/MacOS`, with the default home) they use the app's login item instead (see The app). A second `appd` for the same home exits at once (`run/appd.lock`). The service log is `logs/appd.log`, capped at 5 MiB; older lines move to `appd.1.log` … `appd.3.log` and the oldest file is dropped.

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

Match replies by `id`, never by order. Codes: `hello_required`, `parse`, `unknown_command`, `bad_request`, `no_such_bot`, `no_such_question`, `no_such_routine`, `invalid_routine`, `no_such_connector`, `invalid_connector`, `sign_in_failed`, `models_failed`, `no_such_grant`, `no_such_note`, `no_such_skill`, `invalid_skill`, `no_such_project`, `invalid_project`, `no_such_workplace`, `invalid_workplace`, `workplace_in_use`, `no_such_catalog_entry`, `catalog_failed`, `catalog_changed`, `no_such_surface`, `surface_failed`, `no_such_prompt`, `queue_full`, `cannot_steer`, `no_such_message`, `not_waiting`, `busy`, `export_failed`, `trash_failed`, `engine_failed`, `engine_error`, `engine_stopped`, `internal`.

| Command | Fields | `data` |
|---|---|---|
| `hello` | `protocol` | `{protocol}` |
| `list_bots` | none | `{bots:[Bot]}` |
| `create_bot` | `name` (1 to 80 characters) | `{bot: Bot}` |
| `set_bot` | `bot`, `name?`, `instructions?` (at most 8000 characters), `stepLimit?` (a whole number, 1 to 1000), `avatar?` (at most 8 characters, such as an emoji), `color?` (`#rrggbb` or empty) | `{}`; what is given changes; `bots_changed` follows. A `stepLimit` is sent to a running engine as `step_limit`. |
| `order_bots` | `bots`: every bot's id once, in the new order | `{}`; `list_bots` lists them so from then on |
| `delete_bot` | `bot` | `{}`: its engine and browser stop, it leaves its project, everything stored for it is forgotten, and its folder and browser profile move to the Trash, as Finder moves them (Put Back works). When its folder cannot be moved, nothing is deleted: `trash_failed` |
| `search` | `text` (1 to 200 bytes), `bot?`, `limit?` (default 50, at most 200) | `{hits: [{bot, seq, kind, snippet}]}`: prompts (`kind` `user`) and replies (`reply`) holding `text` in any case, of `bot` or of all bots, newest first. A reply is searched whole, as the chat joins its pieces; `seq` is its first piece's, the chat item's `id`. |
| `open_bot` | `bot`, `limit?`, `before?` | `{bot: Bot, history:[Entry], more, questions:[question event], queue:[{nonce, text}]}`: the last `limit` entries (all without it) with `seq` below `before`, oldest first; `more` is `true` when there are older ones; `queue` is the prompts waiting for the bot (see Queue and steer) |
| `prompt` | `bot`, `nonce`, `text` | `{}` once the engine admitted it; `{queued:true}` when the bot is busy or has prompts waiting (see Queue and steer); `{duplicate:true}` or `{duplicate:true, uncertain:true}` for a known nonce (see Prompt ledger), `{duplicate:true, queued:true}` for one still waiting. `queue_full` when 20 prompts already wait. |
| `answer` | `bot`, `question`, `choice` | `{}` once the engine took it; `no_such_question` if that question is not open. `Always` on a question about an action: see Permissions. |
| `steer` | `bot`, `text` | `{}` once the engine took it; `{queued:true}` when there was no turn to steer or it ended first (see Queue and steer). `cannot_steer` for a bot run by an installed agent. |
| `list_queue` | `bot` | `{queue:[{nonce, text}]}`, in the order they go |
| `remove_queued` | `bot`, `nonce` | `{}`; `no_such_prompt` if that prompt is not waiting (it may have gone to the engine already) |
| `list_mail` | `bot` | `{mail:[{seq, from, to, text, state, answer?, error?, atMs}]}`: the last 50 messages between this bot and other bots, oldest first; `state` is `waiting`, `working` or `answered` (`error` instead of `answer` when none came) |
| `list_room` | `projectId` | `{room:[{seq, from, to, text, atMs}]}`: the last 200 lines of the project's room (see Room), oldest first; `from` is a bot's id, empty for the person; `to` the bot it was addressed to, empty for a bot's reply. `no_such_project` |
| `send_room` | `projectId`, `text` | `{seq, to, queued}`: the person's message, to the bot its `@name` names, else to the project's lead (see Room); `queued` is `true` when that bot was busy or had prompts waiting. `no_such_project`, `no_such_bot` (the name is no bot of the project), `bad_request` (nothing after the name, or the project has no bots), `queue_full` |
| `cancel_mail` | `seq` | `{}`; the sender is told the message got no answer. `no_such_message`; `not_waiting` once the bot took it |
| `abort`, `compact` | `bot` | `{}` |
| `set_model` | `bot`, `model` (empty: back to the engine's default, with no `effort`), `effort?` (`minimal`, `low`, `medium`, `high`) | `{}`; kept for the bot, so each new engine of it starts with them; `bots_changed` follows |
| `rewind` | `bot`, `entry` (a prompt's `nonce`), `fork` | `{}` once the engine went back. Without `fork`, the service then removes that prompt and what followed from the bot's history and sends `history_changed`. With `fork` the bot continues on a copy, the engine keeps the old conversation file, and the history keeps everything: the service appends `{"type":"fork","nonce"}` to the transcript, and the prompts from `nonce` up to that entry are for reading only (the bot no longer has them, and they cannot be rewound to). The removed prompts' nonces stay used in the ledger, so a late retry of one is answered `duplicate` and does not undo the rewind: send new prompts with new nonces. |
| `list_routines` | `bot` | `{routines:[Routine]}` |
| `set_routine` | `bot`, `routine` | `{routine}`. No `routine.id`: create; with one: replace |
| `delete_routine` | `bot`, `routineId` | `{}`. Its past runs go with it. |
| `list_routine_runs` | `bot`, `routineId` | `{runs:[RoutineRun]}`, newest first, at most the last 50 |
| `list_connectors` | `bot` | `{connectors:[Connector]}` |
| `set_connector` | `bot`, `connector` | `{connector}`. Creates or replaces by `name` |
| `delete_connector` | `bot`, `name` | `{}` |
| `sign_in_connector` | `bot`, `name` | `{url}`: open it in the browser. `sign_in_failed` when the server cannot be reached or does not say how to sign in |
| `sign_out_connector` | `bot`, `name` | `{}`; the service forgets the tokens |
| `list_projects` | none | `{projects:[Project]}` |
| `set_project` | `project` | `{project}`; creates when `project.id` is missing. `invalid_project`, `no_such_project`. Refused (`forbidden`) for a device on the network |
| `delete_project` | `projectId` | `{}`: the project is forgotten; its folder and its bots stay. `no_such_project`. Refused (`forbidden`) for a device on the network |
| `list_workplaces` | none | `{workplaces:[Workplace]}` (see Workplaces). Looks at the files now: a workplace that changed is switched off everywhere first |
| `preview_workplace` | `path` | `{manifest, downloadBytes, hosts}` of the folder at `path`: what installing it would fetch, to show before it is installed (`hosts` is `host[:port]`, each once). `invalid_workplace`. Refused (`forbidden`) for a device on the network |
| `install_workplace` | `path`, `download?` | `{workplace}`; copies the folder at `path` in, in place of one of the same name, and fetches what its manifest lists under `downloads`: that is refused (`download_not_agreed`, nothing contacted) unless `download` is `true`, which the window sends after showing `preview_workplace`'s size and hosts to the person. Answers when it is all in place; `workplace_progress` frames come meanwhile. `invalid_workplace`; `download_failed` (what failed; nothing is installed or left behind); `workplace_in_use` when a project has that name on; `busy` while another install runs. Refused (`forbidden`) for a device on the network |
| `list_catalog` | none | `{catalog:[Entry]}`: the entries of the catalog (see Workplaces) read now over HTTPS, each `{name, description, version, url, size, sha256, installedVersion, update}`: `installedVersion` is the version installed under that name (`null`: none), and `update` is true when something is installed and is not the archive the entry pins (a workplace installed from a folder has no pin, so it is offered the entry). `catalog_failed` when the list cannot be read or is not valid. Answers when the network does. Refused (`forbidden`) for a device on the network |
| `preview_catalog` | `name` | `{entry, manifest, downloadBytes, hosts, newly}` for the entry `name`: the service downloads its archive, checks it, unpacks it in `staging/` and reads its manifest, which is shown before anything is installed (nothing is kept). `newly` is `{folders, hosts, commands, downloads}`: what the manifest needs that the installed workplace of that name did not (all of it when none is installed); `downloadBytes` and `hosts` are as for `preview_workplace`. `no_such_catalog_entry`; `catalog_failed` (the list or the archive cannot be had, or the archive is not what the entry pins). Refused (`forbidden`) for a device on the network |
| `install_catalog` | `name`, `sha256`, `download?` | `{workplace}`: installs the entry `name` as `install_workplace` installs a folder, from the archive pinned by the entry (an update is this command too, and only this command applies it). `sha256` is the one `preview_catalog` showed: if the catalog lists another archive now, nothing is downloaded (`catalog_changed`). The archive's size and hash must be the entry's, and its manifest the entry's name and version (`catalog_failed` otherwise; nothing is installed or left behind). Then it goes on as `install_workplace`: `download` and `download_not_agreed`, `workplace_progress` frames, `invalid_workplace`, `download_failed`, `busy`. `workplace_in_use` (before anything is downloaded) while a project has the workplace on. The workplace keeps the version and the archive's hash it was installed at (`archive` in `Workplace`). `no_such_catalog_entry`. Refused (`forbidden`) for a device on the network |
| `approve_workplace` | `name` | `{}`: records the files as they are now and starts the workplace again in the projects that have it on. `no_such_workplace`, `invalid_workplace`. Refused (`forbidden`) for a device on the network |
| `remove_workplace` | `name`, `force?` | `{}`: forgets the workplace and deletes its folder. `workplace_in_use` while a project has it on, unless `force` is true, which switches it off in those projects first; `project-data/<id>/` stays. `no_such_workplace`. Refused (`forbidden`) for a device on the network |
| `list_surfaces` | `projectId` | `{surfaces:[{uri, name, description?, kind}]}` (see Surfaces); empty for a project with no program. Starts the program if it is not running. `no_such_project`, `surface_failed`. Refused (`forbidden`) for a device on the network |
| `read_surface` | `projectId`, `uri`, `offset?`, `limit?` | One part of a surface's content (see Surfaces). `no_such_project`, `no_such_surface`, `surface_failed`. Refused (`forbidden`) for a device on the network |
| `act_surface` | `projectId`, `uri`, `tool`, `target?`, `text?` | `{ok, text}`: the person's action, the call of the program's tool `tool` (see Surfaces). `surface_failed` when the surface does not offer `tool`. Refused (`forbidden`) for a device on the network |
| `watch_surfaces` | `projectId` or `null`, `uri?` | `{}`: this window shows the surfaces of that project, and has `uri` open, or none (`null`); it gets `surfaces_changed` and `surface_changed` for those only. Refused (`forbidden`) for a device on the network |
| `read_roles` | `paths` | `{roles:[{path, name, instructions, model, effort, skills, levels, raises}]}`: what each role file (whole paths, 1 to 20) would create; `raises` names the kinds whose level allows more than a new bot's. `invalid_role` with the file and the reason. Refused (`forbidden`) for a device on the network |
| `add_roles` | `projectId`, `paths`, `raise` | `{bots:[id]}`: a bot for each role, in the project; levels in `raises` are applied only with `raise: true`. All or none: `invalid_role` or `no_such_project` and no bot is created. Refused (`forbidden`) for a device on the network |
| `list_skills` | `bot` | `{skills:[{name, description}]}`, by name |
| `install_skill` | `bot`, `path` | `{skill}`; copies the skill folder at `path`, or `invalid_skill` |
| `delete_skill` | `bot`, `name` | `{}`, or `no_such_skill` |
| `list_memory` | `bot` or `projectId` | `{notes:[{name, bytes, modifiedMs}]}`, by name; with `projectId` the project's notes, or `no_such_project` |
| `read_memory` | `bot` or `projectId`, `name` | `{text}`, or `no_such_note` |
| `write_memory` | `bot` or `projectId`, `name`, `text` | `{}`; creates or replaces the note |
| `delete_memory` | `bot` or `projectId`, `name` | `{}`, or `no_such_note` |
| `set_disk_access` | `bot`, `wholeDisk` | `{}`; see Folder limit. `bad_request` for `true` while the bot has a network limit. Refused (`forbidden`) for a device on the network |
| `set_network` | `bot`, `hosts` | `{}`; `hosts` is a list of at most 50 hosts or `null`; see Network limit. `bad_request` for a host that is not one, or a list for a bot with the whole disk; `busy` in a turn when this switches the limit on or off. Refused (`forbidden`) for a device on the network |
| `hand_back_browser` | `bot` | `{}`; the user gives the bot's browser back (see Browser) |
| `get_helper` | none | `{installed, path, connected, accessibility, screenRecording, fullDiskAccess}`: Bate Helper (see Desktop). The three permissions are `null` until the helper has connected. |
| `open_helper_settings` | `pane` (`accessibility`, `screen_recording` or `full_disk_access`) | `{}`: opens that pane of System Settings, starting the helper first if it is not running, which is what makes macOS list it there. Not for devices on the network. Code `bad_request` for another pane |
| `set_agent` | `bot`, `command` | `{}`; `command` is a list (program, then arguments) or `null`; see Installed agents |
| `get_permissions` | `bot` | `{levels: {read, write, run, connector, desktop, apple_read, apple_change}, grants:[Grant]}` |
| `set_permission` | `bot`, `kind`, `level` | `{}`. `always` for `apple_change` is refused (`bad_request`): see Apple apps |
| `delete_grant` | `bot`, `kind`, `target` | `{}`, or `no_such_grant` |
| `attach` | `bot`, `data` (base64), `name` (first part) or `upload` (later parts), `last?` | `{upload}` while `last` is not true, then `{path}`: the file, at most 25 MiB, is in `attachments/` in the bot's folder under `name`, or `1-name`, `2-name`… when taken. For a bot in a project `path` is full, so a tool resolves it in the bot's own folder; otherwise it stays relative as before. Parts go with the window that sent them; at most 4 files at once per window. Nothing is written through a link in the bot's folder. |
| `export_chat` | `bot` | `{path}`: the whole chat as Markdown in `exports/` under the service's home |
| `export_report` | none | `{path}`: a new bug-report archive (see below) |
| `get_provider` | none | `{url, hasKey, fromEnv}`: the model server's URL (`null`: none) and whether it has an API key. `fromEnv`: nothing is saved, so both are the service's `BATE_MODEL_BASE_URL` and `BATE_MODEL_API_KEY`. Refused (`forbidden`) for a device on the network, as is `set_provider`. |
| `set_provider` | `url` (empty: clear), `key?` | `{}`: sets or deletes the model server URL and optional API key in the service. `url` is `http://` or `https://` with a host and no user, password, query or fragment. Without `key`, or with an empty one, the saved key stays if the scheme, host and port are written the same; `key: null` removes it. A key from the environment is never saved. |
| `list_models` | none | `{models:[String]}`: models reported by `<url>/models`, or empty if unreachable. Signed in to ChatGPT: the models the account is offered, read after renewing the sign-in if its token is at its end; `models_failed` when they cannot be read |
| `get_update` | none | `{state, current, available, error}`: `state` is `off` (the build has no update key), `current`, `checking`, `ready` (a release is downloaded and verified), `waiting` (an install was asked for and waits for the replies to end) or `error`. `available` is the version that is `ready` (or `waiting`), else `null`; `error` is why the last check or install failed. A device on the network may read it |
| `check_update` | none | reads the feed now, and answers with `get_update`'s shape when that is done; refused with `update_failed` and the reason when the release fails a check, or `updates_off` |
| `install_update` | none | `get_update`'s shape, with `state: "waiting"`: when no bot has a turn running (or a compaction), the programs are swapped and the service restarts. Code `no_update` when none is `ready`, `updates_off`. Refused (`forbidden`) for a device on the network |
| `get_chatgpt` | none | `{signedIn}`: whether bots run on a ChatGPT plan |
| `sign_in_chatgpt` | none | `{url}`: the address to open in the browser. The service listens on `127.0.0.1:1455` (another port when taken) for 10 minutes; when the browser comes back, or the sign-in fails, `chatgpt_changed` is sent, with `error` when it failed. Code `sign_in_failed` |
| `sign_out_chatgpt` | none | `{}`: forgets the tokens, keeps the registration, and asks OpenAI to end the session (one try; a failure is logged). Sends `chatgpt_changed` |
| `lan_status` | none | `{on, port?, urls?, tailscaleUrls?, fingerprint?, devices:[Device]}`: whether the LAN client is serving, its URLs on local networks, its URLs on the user's Tailscale network (the MagicDNS name, then 100.64.0.0/10 addresses), its self-signed certificate's SHA-256 fingerprint, and paired devices |
| `set_lan` | `on` | `{}` with the new `lan_status`; turns the LAN client on or off and remembers it in `lan/on` |
| `pair_device` | none | `{code, expiresAt}`: opens a 6-digit pairing code that lasts 5 minutes or 5 tries; only one open at a time |
| `remove_device` | `device` (an id) | `{}`: forgets the device, ending its active sessions immediately |
| `get_channels` | `bot` | `{channels: [{kind, hasToken, pairing, chats: [{chat, name}], error}]}`: one entry for `telegram` and one for `discord`. The token itself is never sent back. `pairing` says whether the code last given by `pair_channel` still lets a chat in. `error` says why the channel is not connected, else `null` |
| `set_channel` | `bot`, `kind`, `token` (empty: remove) | `{}`: saves the bot token of the channel and connects, or removes the channel and its paired chats. Code `token_in_use` when another bot has the same token |
| `pair_channel` | `bot`, `kind` | `{code, expiresAt}`: a 6-digit code that lets one chat in; it lasts 5 minutes or 5 wrong guesses. Code `no_channel` without a token |
| `remove_chat` | `bot`, `kind`, `chat` | `{}`: the chat is no longer let in. Code `no_such_chat` |

`Bot` is `{id, name, createdAt, running, busy, wholeDisk, network, agent, browserTaken, instructions, stepLimit, avatar, color, model, effort, project}` (`model` and `effort` only once chosen) (`agent` only when set; `browserTaken` only when true; `project`, `{id, name}`, only for a bot in a project): `stepLimit` is the tool rounds a turn may take, 100 unless set; `running` while the bot has an engine, `busy` between `turn_start` and `turn_end`, and while a `compact` is unanswered. `bots_changed` is sent each time it changes, and `projects_changed` each time `project` does. Every engine operation starts the bot's engine if it has none; the service then sends `start`, or `open` with the bot's last `session`. If the engine refuses `open`, the service forgets that session and sends `start`.

A `Bot` also has `waiting`: `true` while one of its questions is open (see Questions), so a list shows which bots need the person without opening each one. `bots_changed` is sent when a question opens and when it closes.

### Events (service → every window that said hello)

| Frame | Meaning |
|---|---|
| `{"type":"transcript","bot","seq","at","entry"}` | An entry was appended to the bot's transcript. |
| `{"type":"bots_changed"}` | Re-run `list_bots`. |
| `{"type":"question_closed","bot","question"}` | The engine took an answer to the question, or the turn that asked it ended. |
| `{"type":"engine_stopped","bot"}` | The bot's engine exited. Its open questions are dropped. |
| `{"type":"history_changed","bot"}` | Entries were removed (`rewind`). Re-run `open_bot`. |
| `{"type":"routines_changed","bot"}` | Re-run `list_routines`. |
| `{"type":"queue_changed","bot"}` | Re-run `list_queue`. |
| `{"type":"mail_changed"}` | Re-run `list_mail`. |
| `{"type":"room_changed","projectId"}` | Re-run `list_room`: a line came into that project's room. |
| `{"type":"connectors_changed","bot"}` | Re-run `list_connectors`. |
| `{"type":"update_changed"}` | Re-run `get_update`. |
| `{"type":"workplace_progress","name","done","total"}` | An `install_workplace` or `install_catalog` has fetched `done` of `total` bytes of the workplace `name`'s downloads (at most four a second, and after each download). |
| `{"type":"chatgpt_changed","error?"}` | Re-run `get_chatgpt`. `error` says why a sign-in failed. |
| `{"type":"permissions_changed","bot"}` | Re-run `get_permissions`. |
| `{"type":"helper_changed"}` | Re-run `get_helper`: the helper app connected, quit, or its permissions changed. |
| `{"type":"channels_changed","bot"}` | Re-run `get_channels`: a channel was set or removed, connected or failed, a chat was paired or removed, or a wrong code was sent. |
| `{"type":"projects_changed"}` | Re-run `list_projects`. |
| `{"type":"workplaces_changed"}` | Re-run `list_workplaces`: one was installed, approved or removed. |
| `{"type":"surfaces_changed","projectId"}` | Re-run `list_surfaces`: the program's list of surfaces changed. Sent only to a window that said `watch_surfaces` for the project. |
| `{"type":"surface_changed","projectId","uri"}` | Re-run `read_surface` for what is open: the program says the surface changed (a bot's tool call, or any other cause). Sent only to a window that has `uri` open (`watch_surfaces`). |
| `{"type":"skills_changed","bot"}` | Re-run `list_skills`. |
| `{"type":"memory_changed","bot"}` or `{"type":"memory_changed","projectId"}` | A window changed a note of the bot or of the project: re-run `list_memory`. What a bot writes itself is not announced. |

### Transcript

The service appends every engine event except `ready` and `response` to the bot's transcript, plus `{"type":"prompt","nonce","text"}` for each prompt the engine admitted and `{"type":"fork","nonce"}` after a fork. When an engine stops mid-turn, the service appends `turn_end` with reason `error`, so history never shows a reply still running. An `Entry` is `{seq, at, entry}`; `seq` grows and `at` is Unix milliseconds. Windows read history from here (`open_bot`), so a window that was closed or disconnected mid-reply sees the whole reply. Engines belong to the service, so closing a window never stops a reply.

### Prompt ledger

Every prompt has a `nonce`. The service writes the nonce to the ledger (`sending`, committed to disk) before the prompt reaches the engine, and marks it `sent` when the engine answers the request. A nonce reaches the engine at most once:

| A `prompt` whose nonce is… | Reply |
|---|---|
| new | sent; `{}` once the engine admits it |
| `sending` or `sent` | `{duplicate:true}`; nothing is sent |
| uncertain: the engine or the service stopped while it was `sending` | `{duplicate:true, uncertain:true}`; nothing is sent. The window should tell the user and let them send it again under a new nonce |

A nonce the engine refused (`engine_error`), or that could not be sent because the engine did not start (`engine_failed`), is forgotten, so it may be sent again.

### Queue and steer

A `prompt` for a bot that is in a turn, has a prompt the engine has not answered, is being compacted, or already has prompts waiting is not sent. The service stores it in its database (so it survives a closed window and a restart), answers `{queued:true}` and sends `queue_changed`. At most 20 prompts wait for a bot; more are refused `queue_full`. Prompts go one at a time, in order: when a turn ends or a compaction is done the first one is sent as an ordinary prompt through the ledger, and it enters the transcript and leaves the queue when the engine admits it; the next goes when that turn ends. A service that starts sends the first waiting prompt of each bot at once. A prompt whose nonce the ledger already knows (an engine stopped while it was `sending`) is dropped from the queue, never sent twice. `remove_queued` takes a waiting prompt back.

`steer` puts a message into the turn that runs; the engine announces it with `steered`, which the service records in the transcript at that point, so the window shows it where the bot took it, and it is part of the saved conversation. A steer that the turn did not take in, because it ended first (the engine refuses it, or ends the turn without taking it, or stops), is not lost: it becomes the bot's next prompt, in front of the others waiting, under a nonce `steer-<random>`. A `steer` while no turn runs is the same: the next prompt. A bot run by an installed agent (`agent` set on the `Bot`) cannot be steered: `steer` is refused `cannot_steer` without asking the agent, and a window offers only `prompt` (which queues) for it.

### Bots together

The bots of one project may message each other (see Projects); no other bots can. The service tells each running engine its bot's peers (`peers`: the other bots of its project, the project's lead marked `lead`) when it starts and after every change to the projects; a bot in no project has none. `leads` is true for the bot that is its project's lead and has at least one other bot in the project.

A message (`message` from an engine) to another bot that shares its project with the sender (not to itself) is stored in the service's database. When the other bot is free (no turn, no unanswered prompt, no prompt of its user waiting, no older message waiting), it goes at once as a prompt with nonce `mail-<seq>` and a first line that says which bot it is from and that it is not the user; what that bot says after its last tool call is the reply, sent back (`reply`) when its turn ends. Otherwise the message waits in the bot's inbox (at most 20) and the sender is answered at once that it waits; the bot takes it after its user's waiting prompts, and its answer comes to the sender later as a prompt of its own (nonce `answer-<seq>`, queued like a user's prompt). Waiting messages survive a restart; a message whose turn was running when the service stopped is answered that the service restarted. No answer, and the reason instead, when: its turn is stopped by its user or fails; its engine stops; its user cancels it (`cancel_mail`); or it says nothing. A reply is cut at 100 000 bytes.

A project's lead differs from its other bots in what its engine tells the model: that it leads, and to hand out tasks and put the results together. It has no way to stop or redirect another bot; only the user stops a bot. Each bot acts with its own permissions and its own folder limit, whoever messaged it, and its questions go to the user as always.

Whether a bot may message another is asked like a connector's tool: a `question` of kind `connector`, target `bot:<id>`. `Always` remembers it for that bot; the `connector` level of the sender applies.

### Projects

Same shape as `proto::projects`. A project is `{id, name, folder, bots, lead, instructions, hosts, connector, workplace, connectorError}`: `name` is 1 to 80 characters, `folder` a folder's full path, `bots` at most 20 bots' ids, `lead` (optional) one of `bots` (`invalid_project` otherwise; the first bot when not set, and also when the bot it names has left), `instructions` at most 8000 characters, `hosts` at most 50 hosts added to what each of its bots with a network limit may reach (see Network limit; `invalid_project` for one that is not a host), `connector` (optional) `{name, command, args}` (see A project's connector), `workplace` (optional) the name of an installed workplace (see Workplaces; not together with `connector`), `connectorError` output only; there are at most 50 projects, and a bot is in at most one. The bots of a project work in its folder, and may message each other (see Bots together); `lead` is the bot that gets the room's messages with no `@name`, and the one told it leads the project.

### Room

A project has a room: one conversation in which the person writes to its bots and the bots' replies come back, in one timeline (the window's Room tab). The project owns it: the service keeps the room's lines in its database (`room`), each with who wrote it (a bot's id, or the person), the bot it was addressed to (none for a bot's reply), the text and the time. Deleting the project deletes its room; a deleted bot's lines stay, under its id (the window shows "(deleted bot)").

- **Addressing:** `send_room` text that starts with `@name` goes to the bot of the project with that name (any case; the longest name that fits, so names may hold spaces; the name must be followed by a space or the end), and the message is what follows. Anything else goes to the project's `lead`. The line is added to the room before anything else, and `room_changed` follows.
- **Delivery:** the message is an ordinary prompt of that bot, nonce `room-<seq>`: it is in the bot's queue and goes as a typed prompt goes (at once to a free bot, else when its turn ends, behind the prompts waiting; `queue_full` at 20), so the bot's own conversation holds it and its reply as it holds any turn.
- **Context:** the prompt starts with what the room said since the bot's last line in it: lines that are not the bot's own and not addressed to it, the last 10 at most and at most 4000 characters (whole lines, newest first; one line alone longer than that is cut), oldest first, each as `<who>: <text>` or `<who> to <bot>: <text>`. Without such lines the prompt is the message alone. The bot sees nothing else of the room, so what it costs in context stays bounded.
- **The reply:** what the bot says after its last tool call, when its turn ends (as for a message from another bot), goes into the room as a line from the bot. A turn that gives none (stopped, failed, its engine gone) leaves a line that says so. The room is not pruned.

- **The folder:** the user names it (`~/` stands for their home). It must be there, and it is kept by its real path. It is refused (`invalid_project`) when it is the user's home or a folder above it, when it holds the service's home, or when it is inside the service's home. With `folder` empty a new project gets `projects/<id>` in the service's home, made by the service, and a project that has a folder keeps it. `delete_project` never deletes a folder.
- **Working there:** the engine of a bot in a project is told the folder in `BATE_WORK_DIR`. Its tools take a relative path from there and run commands there. The bot's own folder stays where its conversations, notes, skills and attachments are; the engine tells the model that folder, and names notes and skills by their full paths.
- **The limit:** a bot kept in its folder may read and change everything in the project's folder as well, and nothing else (see Folder limit). The folder is checked again each time an engine starts; one that is gone or no longer allowed leaves the bot without an engine (`engine_failed`) until the project is changed.
- **Changes:** the limit is set when an engine starts, so `set_project` and `delete_project` stop the idle engine of every bot that came, went, or whose project's folder changed; a bot in a turn finishes under its old rule and stops then. Its next turn starts under the new rule. The window's project-file link changes with the engine, never during its turn. `projects_changed` follows, and every running engine gets its peers for its next turn.
- **Files in the window:** `work/<bot>` in the service's home is a link the service keeps to the folder of the bot's project. For `botfile://localhost/<bot>/<path>` the window looks in that folder first, then in the bot's own.
- **Instructions and notes:** the engine of each bot of a project is sent, as its `instructions`, the bot's own followed by the project's, again whenever either changes; it follows them from its next turn. The project's notes are the Markdown files in `.bate/memory/` in its folder, under the same rules as a bot's own (see Memory); the engine puts them before the model after the bot's own, with a budget of their own, and the bots add notes there with their `write` tool. An installed agent (see ACP) gets neither.
- **Roles:** a role file describes a ready-made bot: name, instructions, permission levels, an optional model and the skills it needs as folders beside it. The format is in `docs/roles.md`. `add_roles` re-reads the files, creates each bot with its instructions, model, skills and levels, then adds them to the project as `set_project` would; when anything fails, every bot it made is taken back.

### Workplaces

A workplace is a folder with a `workplace.json` and a program (format and example in `docs/workplaces.md`; the idea in `docs/adr/0001-workplaces-are-separate-programs.md`). `install_workplace` validates the manifest, copies the folder as a skill is copied (into the service's `staging/`, no links or special files, at most 2000 files and folders, 200 MiB and 12 folders deep; `invalid_workplace` otherwise) and moves it to `workplaces/<name>/` in the service's home, which no bot or program can write. A program named by a path in the folder must be there and runnable. The service records the name, the version and a fingerprint of the files in its database. Installing a name again replaces it, unless a project has it on (`workplace_in_use`).

`Workplace`, as `list_workplaces` gives it: `{name, version, manifest, skills, roles, changed, error?, usedBy, archive?}`. `manifest` is `{name, version, description?, program: {command, args}, needs: {folders, hosts, commands}, downloads?: [{url, size, sha256, dest, unpack?}]}`, or `null` when the manifest is malformed (`error` says how, or that the files cannot be read). `needs` is shown to the person before a workplace is switched on and stored; the hosts are enforced (see Network limit). `skills` are the valid skills in its `skills/` (`{name, description}`), `roles` the whole paths of its `roles/*.md` files, which `read_roles` and `add_roles` take like any role file (a role's skills are folders beside the role file). `changed` is true when the files differ from what was approved. `usedBy` is the ids of the projects that have it on. `archive` is the SHA-256 of the catalog's archive it was installed from (absent for one installed from a folder); `approve_workplace` leaves it as it is.

- **Downloads:** `manifest.downloads` is a list of at most 20 entries `{url, size, sha256, dest, unpack?}`. `url` is `https`, or `http` to this Mac (`127.0.0.1`, `localhost`), with no user name; up to 5 redirects are followed (GitHub serves release files from another host), so a host shown may send the service on to another, and the hash, not the host, makes the file trusted. `size` is 1 byte to 16 GiB, the most the download may be; `sha256` is 64 lowercase hex digits; `dest` is a path inside the workplace's folder (not absolute, no `..`, no other name in it), different for each entry and not already there (the workplace's own files are never overwritten). Without `unpack` the file is written at `dest`; with `unpack: "tar.gz"` or `"zip"` the archive is unpacked into the folder `dest` by `/usr/bin/tar` or `/usr/bin/unzip` (the system tools; `tar` is what the updater uses too). `install_workplace` fetches them one after the other into `staging/`, writing and hashing each as it arrives (the code the updater uses, `download.rs`), and stops at the first failure: a body over `size` (by `Content-Length` or on the stream), a different length, a hash that differs, more than 5 redirects, an HTTP error or a broken connection ends the install, and the staging folder, with everything fetched so far, is removed. Nothing is resumed: installing again starts over. An archive is unpacked only after its hash matched, and only if its listing shows nothing but plain files and folders (no link, hard link, device or pipe), each with a path of normal names (not absolute, no `..`), and at most 102000 entries, at most 32 folders deep below `dest`; otherwise nothing is unpacked. The downloaded files are part of the workplace's folder and in the fingerprint, which counts at most 102000 files and folders and 44 levels: the workplace's own limit plus a limit for what is downloaded (100000 files and folders, 32 levels). A workplace carries no signature: its downloads are pinned by the hashes in the manifest, which the person installs, and nothing a workplace supplies is read as an update (the update feed comes only from the service's own address, `BATE_UPDATE_FEED` or the default, and a feed is accepted only with a signature over `bate-update\n<version>\n<sha256>` by the compiled-in key).
- **The fingerprint:** SHA-256, as 64 hex characters, over the folder's entries, each folder's in order of name, depth first: the path from the workplace's folder, a zero byte, the kind, and for a file whether it can run, its length (8 bytes, little-endian) and the SHA-256 of its bytes. A link or a special file counts as such, so one added later changes it. The service reads a file's bytes again only when its inode, length, modification time or change time moved since it last read them, so a check of an unchanged workplace reads no file.
- **Switching on:** `set_project` with `workplace: "<name>"`. It is refused (`invalid_project`) when the workplace is not installed, its manifest is malformed, its files changed since they were approved, or the project also has a `connector`. The window shows `needs` and asks for a confirmation before it sends this; the service does not. Switching a workplace on or off, or changing it, stops the idle engines of the project's bots (a busy one when its turn ends), as any change of the folder limit does.
- **The program:** it is the project's connector under the workplace's `name`: started once per project, with `project-data/<projectId>/` as its data folder, `BATE_MCP_SOCKET`, `HOME`, `TMPDIR` and `BATE_WORK_DIR` as for a project's connector, but its working directory is the workplace's folder, where a relative `command` and the `args` start. A `command` without `/` is looked up on the service's `PATH` (`python3`). It may read its own folder and the project's; it may write the data folder and the project's folder.
- **Skills and roles:** while the workplace may run, the engine of each bot of the project is sent `skill_folders` with the workplace's folder, may read it, and tells the model its skills beside the bot's own (a skill of the bot's own with the same name stays listed too, each with its own path). Nothing is copied into the bot's folder.
- **Changed files:** the files are compared with the fingerprint when the program starts (including each start after it ended) and when `list_workplaces` runs, and a mismatch, or a manifest that no longer parses, switches the workplace off in every project that has it on: the program stops, `connectorError` says so and that the person has to look and approve, the bots get no tools from it (`connectors`) and no skills (`skill_folders` with none) from their next turn, and the project and its bots go on. `approve_workplace` records the files as they are (after checking the manifest again) and starts the program again. A new version is another matter: while a project has the workplace on, `install_workplace` is refused, so switch it off in the project, install, and switch it on. A change made while the program runs is noticed at the next of those two moments, not at once.
- **A program that does not start:** reported in `connectorError` as for any project connector; the project opens and its bots answer without its tools.
- **The catalog:** `catalog/workplaces.json` in Bate's repository, read from `https://raw.githubusercontent.com/d41sys/bate/main/catalog/workplaces.json` (`BATE_CATALOG` names another address; it is a test's stand-in) when the window asks (`list_catalog`, `preview_catalog`, `install_catalog`), never by itself. It is at most 1 MiB and `{"workplaces": [Entry]}` with at most 200 entries `{name, description, version, url, size, sha256}` (format in `docs/workplaces.md`); a list with a bad entry or a name twice is refused whole. An archive is read like a workplace's download (at most 5 redirects, no more than `size` bytes, size and hash checked) into `staging/`, unpacked with the checks of a download's `tar.gz` (plain files and folders only, none outside the folder), and must hold the entry's workplace and version; then the folder is installed as `install_workplace` installs it, so the manifest's own downloads, limits and the consent for them apply. There are no signatures: the size and hash in the list, which the person saw the archive's manifest for, are what pin the archive. A newer entry is never applied by itself.
- **Removing:** see `remove_workplace`. Deleting a project, or switching a workplace off in it, stops the program but never deletes `project-data/<projectId>/`.

### Surfaces

A surface is a table, a tree or a code view that a project's program fills and that the window draws itself, as text, in its pane beside the conversation. The program offers it as an MCP resource; there is no second protocol. The service is the MCP client of the program here (as a program's `initialize` sees it, a client that names itself `bate` and offers no capabilities), over the same socket the bots' calls come through, and the window never reaches the program. A program with no `resources` capability has no surfaces.

- **The kinds:** a resource whose `mimeType` is `application/vnd.bate.table+json`, `application/vnd.bate.tree+json` or `application/vnd.bate.code+json` is a surface of that kind (`table`, `tree`, `code`); other resources are not listed. `list_surfaces` follows `resources/list` (`nextCursor`: at most 10 pages), keeps at most 100 surfaces, and gives `uri`, `name` (at most 200 bytes), `description?` (500) and `kind`.
- **Content:** `resources/read` answers with one `text` content that is a JSON object. Everything in it is data: each string is shown as text, never as markup, script or a link; a number, boolean or other value is shown as its JSON text, `null` as nothing. Besides the fields of each kind, any of them may name what the person can do: `select` and `open`, each the name of a tool, and `actions`, a list of `{tool, label}` (at most 20).

  | Kind | Fields |
  |---|---|
  | table | `columns`: the headings (at most 20); `rows`: `[{id?, cells: [...]}]` (at most 20 cells; `id` is the row's place in the whole when missing) |
  | tree | `nodes`: `[{id?, label, depth}]` in the order of a depth-first walk, where `depth` is 0 for a top node (at most 64) and a node's children are the deeper ones that follow it |
  | code | `lines`: the lines of text, `language?`: a name to show (at most 40 bytes) |

- **Parts:** content is read in parts and the service sets the size. `read_surface` takes `offset` (default 0) and `limit` (default 100, at most 500) in rows, nodes or lines, and asks the program for that part with `params._meta["bate/part"] = {offset, limit}` on `resources/read`. A program that honours it sends only those items and `offset` (where they begin in the whole) and `total` (how many the whole has). A program that does not sends everything and no `offset`: the service cuts the part out. Either way the service gives at most `limit` items, stops earlier when the part passes 512 KiB, cuts a cell, label or line at 1000 (a line 2000) bytes, and reads at most 16 MiB of an answer, so a program cannot make a window load more. The answer is `{kind, offset, total, <rows|nodes|lines>, columns?, language?, select?, open?, actions}`; `total` is never less than what the part reaches.
- **Changes:** a program that says `resources.subscribe` is sent `resources/subscribe` for a surface the first time it is read, and the service reads its `GET /mcp` stream of server-sent events (as MCP's HTTP transport has it; at most one, restarted once a second until the program is gone or offers none) for `notifications/resources/updated` and `notifications/resources/list_changed`. Each becomes a `surface_changed` or `surfaces_changed` frame for the windows that `watch_surfaces` named the project (and, for `surface_changed`, the surface), and nobody else: a device on the network never. The frame holds no content; the window reads again. Every window reads from the one program, so two windows on a project show the same content. A program that cannot tell of changes is read again by a window after that window's own actions.
- **Actions:** `act_surface` first reads the surface's first item to see what it offers, and refuses (`surface_failed`) a `tool` that none of `select`, `open` and `actions` names, so the window cannot call any other tool of the program. Then it sends `tools/call` with `name` `tool` and `arguments` `{uri, target?, text?}`: `target` is the id of the row or node the person chose, or the number of the line (from 1); a named action takes the chosen one, if any. An action the content gives as `{tool, label, input: true}` takes words: the window shows a field for them and sends them as `text` (cut at 2000 bytes). The answer is `{ok, text}`: `ok` is false when the tool says `isError`, and `text` is what it said (at most 2000 bytes). A choice is `select`, a double click or Enter is `open`. The result is the program's own change of the content, which it announces, and the window reads again after the call.
- **Where:** the window offers a project's surfaces under the conversation of a bot of the project (Bate's own window on this Mac only; the web client on the local network has no surfaces, and the service refuses these commands to it) and draws the chosen one in the pane, one part at a time. A program that is not running is started by `list_surfaces`; one that failed to start is reported as `surface_failed` with the reason `connectorError` gives.

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

The memory commands are for the window's panel; with `projectId` they work on the project's notes instead of a bot's. A `name` is a file name ending in `.md`, at most 100 bytes, without `/` and not starting with a dot (`bad_request` otherwise); `text` is at most 64 KiB. The bot can write in its folder, and every bot of a project in the project's, so the service trusts nothing there: only plain files directly in a plain `memory` folder (for a project, in a plain `.bate` folder) are listed, read or replaced, never what a link points to (a file with a second name elsewhere counts as one), and the engine shows the model exactly the notes `list_memory` lists.

### Folder limit

A bot is kept inside its folder (`bots/<id>` under the service's home), and the folder of its project if it is in one, by the operating system unless `wholeDisk` is true; a new bot is limited. The service starts a limited bot's engine under macOS `sandbox-exec`, so the rule holds for the engine and for every program it starts, whatever was approved:

- **Writing:** only inside the folder and the project's folder.
- **Reading:** nothing under `/Users`, `/Volumes`, `/private/var/folders`, `/private/tmp`, `/cores`, Homebrew's `var`, the system's logs or the service's home, except the folder, the project's folder, the engine's program and, for the engine of a bot whose project has a workplace and for the workplace's own program, that workplace's folder. System files stay readable.
- **Other programs:** no Apple events, no opening apps, no launchd jobs, no signals to or looks at processes outside (so no `pgrep`), no drivers, no shared memory, no local sockets outside the folder (so not the service's own socket, nor the user's key agent), no preferences (`defaults` would write into the user's home), and only the system services needed to look up names and check certificates (so not the keychain, the clipboard or the file index).
- **Environment:** `PATH`, `LANG`, `LC_ALL`, `USER`, `LOGNAME`, `SHELL`, `TERM` and the engine's own `BATE_` settings except `BATE_MODEL_API_KEY`; nothing else of the service's. `HOME` is the folder and `TMPDIR` is `.tmp` in it, so programs keep their settings and temporary files there.
- **Network:** open unless the bot has a network limit (see Network limit), local ports included: the engine has to reach its model.

`set_disk_access` stores the choice and stops the bot's idle engine; the next thing the bot does starts an engine under the new rule. `bots_changed` follows. During a turn it is refused with `busy`. On a system without `sandbox-exec` a limited bot's engine does not start (`engine_failed`).

Not covered: what a bot sends over the network from its own folder, unless it has a network limit (below); services listening on local ports; anything a bot started while it had the whole disk. A limited bot cannot use programs installed under `/Users` (a connector's command there cannot be read), and a program that insists on the system's own temporary folder (`mktemp` with no template, Xcode's tool cache) is refused there.

Whatever the limit, no engine gets `BATE_MODEL_API_KEY` in its environment, and a command or connector the model starts does not get it either.

### Network limit

A bot's commands, and a workplace's program, can be held to a list of hosts by the operating system and a proxy of the service, so a tool cannot get round it by ignoring proxy settings.

- **Who:** a workplace's program is always held to the `needs.hosts` of its manifest, which the program cannot widen (a program of a project's own `connector` is not held). A bot is held when `set_network` gives it a list (`hosts: []` is: no host); `hosts: null` is open, which is what every bot starts as. `bots_changed` follows; the bot's `network` is the list or `null`. The limit is the system's, so it needs the folder limit: a bot with the whole disk gets none (`bad_request`), and one with a limit cannot be given the whole disk.
- **Hosts:** `host`, `*.domain` (its subdomains, not `domain` itself), or either with `:port`; no port means 80 and 443. Lower case, at most 50; an IPv6 address is not a host. A bot may reach its own list, its project's `hosts` (`set_project`), and its model server (the engine's model calls go through the same proxy: the limit is on its commands, not its model). A list is read for each connection, so a change of it, or of the project's, applies at once; switching the limit on or off stops the bot's idle engine (the system's rule is set when one starts) and is refused with `busy` during a turn.
- **How:** the service listens on a port of its own on `127.0.0.1` for an HTTP proxy: `CONNECT host:port`, and a plain `http://` request. The limited process runs under `sandbox-exec` with every outgoing connection refused but those to that port, to the service's local port (for an engine's connectors) and to unix sockets in its folder (so a workplace's program still serves `BATE_MCP_SOCKET`); names are not looked up by it, the proxy does it. It gets `HTTP_PROXY`, `HTTPS_PROXY`, `ALL_PROXY` and their lower-case names, with a key per process as the proxy's user name, which tells the proxy whose list is meant. A name that only a `*.domain` rule matched is never connected to on this Mac's own addresses.
- **Refused:** a host not in the list gets `403` from the proxy (`407` for a key it does not know) and is never contacted. A tool that ignores the proxy settings cannot connect at all (`Operation not permitted`): the limit fails closed. If the proxy's port cannot be opened, a process that is to be limited does not start.

Not covered: a bot's browser, which the service runs; what a limited bot's commands send to its model server, which they can reach; anything done with the whole disk.

### Permissions

Same shapes as `proto::permissions`. For each bot and each kind of action (`read`, `write`, `run`, `connector`, `desktop`, `apple_read`, `apple_change`) there is a level: `always`, `ask` or `never`. A bot starts with `read` on `always` and the rest on `ask`.

The service decides every engine `question` that has a `kind`, on the `target` the engine is about to run:

- `always`: it answers `Approve` itself. `never`: it answers `Deny` itself, whatever was granted before. Neither is shown or stored as a question.
- `ask`: if a grant has exactly this `kind` and `target`, it answers `Approve` itself. Otherwise the question goes to the windows with the options `Approve`, `Always`, `Deny`.

`answer` with `Always` gives the engine `Approve` and, once the engine has taken it, stores a grant `{kind, target, atMs}` for that exact action. `Approve` is for this once. Grants are listed by `get_permissions` and removed by `delete_grant`; `permissions_changed` follows each change. `Always` on a question without a `kind` is refused (`bad_request`). A `connector` grant is for the tool, whatever its arguments, and goes when its connector is changed or removed (a connector's name has no `__`, so `<connector>__<tool>` names one tool). A target is compared as written: `a.txt` and `./a.txt` are two actions.

### Questions

An engine `question` is stored until the engine takes an `answer` to it (an answer the engine refuses leaves it open), its turn ends, or the engine stops. `open_bot` lists the open ones, so a window that reconnects still sees them.

### Routines

Same shapes as `proto::routines`: a routine is a prompt sent on a daily, weekly, monthly or yearly schedule at a wall time in an IANA zone, every `interval` periods counted from `anchor`. Two more frequencies count real time, starting at `time` on the anchor date: `hourly` (every 1 to 99 hours) and `minutes` (every 5 to 1440 minutes); a clock change neither skips nor repeats their runs. The service sets `id`, `anchor`, `nextRunMs` and `lastRun`. `name` is 1 to 80 characters, `prompt` 1 to 8000, and a bot has at most 50 routines.

At a slot the service sends `"[Routine: <name>]\n<prompt>"` with nonce `routine-<id>-<slotMs>`, through the ledger, so a crash cannot send it twice. A slot missed while the service was down runs once if it is at most 6 h old; an older one is recorded as `missed`. Every handled slot is kept as a `RoutineRun` (`atMs`, `outcome` `sent`, `failed`, `limit` or `missed`, `error`); `routines_changed` is sent when one is added. A run whose turn then ended with `turn_end` reason `limit` (the bot's step limit) is changed to `limit`, with the engine's `error` ("stopped after N tool rounds"); this holds for a scheduled run, a webhook and `/run` from a chat. A wall time that does not exist (clocks jump forward) runs when the gap ends; one that happens twice runs at the earlier instant.

A routine may leave out `schedule` when `webhook` or `chat` is true, and may have all three; one with none is refused (`invalid_routine`). `enabled: false` turns off all of them.

With `chat: true`, a chat paired on one of the bot's channels starts the routine by writing `/run <name>` (the name in any case). The lines after the first go below the prompt as data, fenced like a webhook's; the nonce is `chat-<id>-<random>`; the run is recorded like a webhook's, and the bot's last words go back to the chat. `/run` with a name that is not such a routine is answered with the names that are.

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

### Connectors

Stored per bot (`proto::connectors`): `name` is 1 to 40 characters of `a-z`, `0-9`, `_`, `-`; `transport` is `stdio` (`command`, `args`, `env`) or `http` (`url` starting with `http://` or `https://`, `headers`). Values are stored as plain text. The service sends a bot's list to its engine when the engine starts and after every change (`connectors`).

#### Signing in (OAuth)

A connector reached over http may set `oauth: true`: the server signs its users in. Its `url` must then be `https` (or `http` on this Mac) and its `headers` may not hold `authorization`. `list_connectors` adds `signedIn` to such a connector. `clientId` is this app's id at a server that does not register apps by itself.

The bot never holds the token. Its engine is sent the connector with `url` replaced by `http://127.0.0.1:<port>/mcp/<key>` (the service's local port, see Webhooks; `key` is 32 random hex characters, made per bot and connector), with no headers, and with `enabled` false until the user is signed in. Signing in or out sends the engine the list anew. The service's proxy there passes each `POST` on to the real `url` with the connector's own headers, `Authorization: Bearer <access token>`, and of the engine's headers only `content-type`, `accept`, `mcp-protocol-version`, `mcp-session-id` and `last-event-id`. The answer's status, `content-type`, `mcp-session-id` and body come back as they arrive (so a stream stays a stream). Redirects are not followed. A request body may be 1 MiB.

`sign_in_connector` finds the sign-in server (`/.well-known/oauth-protected-resource`, then `/.well-known/oauth-authorization-server` or `/.well-known/openid-configuration`; every address must be https; one on this Mac is taken only when the connector's own `url` is on this Mac; none may hold a user name), registers this app there unless `clientId` is set, and answers with the address to open: an authorization request with PKCE (`S256`), a random `state` and `resource` set to the connector's `url`. The browser comes back to `http://127.0.0.1:<port>/oauth/callback`; the service trades the code for tokens, stores them and sends `connectors_changed`. A `state` works once, for 10 minutes.

An access token within 30 s of its end is renewed before use; an answer of 401 from the server renews it once and sends the request again. Without tokens, or when renewing fails, the proxy answers 401 and the server is not asked. When the sign-in server refuses to renew, the tokens are forgotten: the connector shows as signed out. Tokens are for one address: changing a connector's `url`, `headers` or `oauth`, or removing it, forgets them. A connector that is off is not served.

Tokens are kept in the Keychain (see Secrets), not in `app.db`. Connector values are plain text in `app.db`. A bot limited to its folder cannot read that file or use the Keychain.

#### A project's connector

(A project that has a `workplace` has no connector of its own: the workplace's program is its connector, with everything below, under the workplace's name. See Workplaces.)

A project may have a `connector`, `{name, command, args}`: `name` follows the rules above (`invalid_project` otherwise), `command` is not empty and is a path (a relative one starts in the project's folder) or a name found on the service's `PATH`. The service runs that program once for the project, and every bot of the project gets its tools, whether it runs on Bate's own engine or on an installed agent that accepts http connectors (see ACP). No engine starts a copy. A program that only speaks over standard input stays a per-bot `stdio` connector.

- **Start:** when the engine of a bot of the project starts, or when `set_project` adds or changes the connector while such an engine runs. The service makes `project-data/<projectId>/` in its home (mode 0700; it stays when the project goes) and starts the program under the folder limit (see Folder limit; and for a workplace's program the network limit), with that folder and the project's in the places of the bot's folder and its project's: it may read and write only those, and read the program itself. Its working directory is the project's folder, `HOME` and `TMPDIR` are in its data folder, and `BATE_WORK_DIR` is the project's folder. `BATE_MCP_SOCKET` is the path of a unix socket in the data folder (at most 103 bytes, else the program does not start) that the program binds and serves MCP over HTTP on: a JSON-RPC message arrives in a `POST /mcp`, and the answer is JSON or an event stream. The program should answer each request on its own connection and honour `connection: close`. Its standard input is a pipe the service holds, so it closes when the service ends, however it ends; its standard error goes to the service's log.
- **Who may call:** a bot never gets the socket's path. Its engine is given `http://127.0.0.1:<port>/mcp/<key>`, a key per bot and connector, as for a connector that signs in; the service sends that POST to the socket with the engine's `content-type`, `accept`, `mcp-protocol-version`, `mcp-session-id` and `last-event-id` and passes the answer back as it comes. The program opens no TCP port. A bot kept in its folder can neither read the service's home nor connect to a unix socket outside its folder (see Folder limit), so a request that does not come through the service cannot reach the program, whatever the program itself does. The limit: a program of the same user that is not sandboxed could connect to the socket, as it could read everything in the service's home.
- **The bot's list:** the connector is in each bot's list under the project's `name` with an `http` transport, and takes the place of a connector of the bot's own of that name. It is `enabled: false` while the program is not running. The window's `list_connectors` shows only what is stored for the bot.
- **Approvals:** as for any connector: kind `connector`, target `<name>__<tool>`, and "Always" is kept per tool. Changing or removing the project's connector, deleting the project, and a bot joining or leaving, forget the Always answers of that connector's name for the bots concerned.
- **Life:** a program that ends is started again after 1, 2, 4, 8, 16, then 30 s (the count starts again after a run of 30 s). It is stopped, with the processes it started, when the project is deleted, when its connector is removed or changed, when the project's folder changes (it starts again with the next engine start), and when the service is asked to stop (`SIGTERM` or `SIGINT`). After a `SIGKILL` of the service only the end of its standard input tells it to stop.
- **Errors:** while the program is not running for a reason, `connectorError` is in the project (`list_projects`, `set_project`): that it cannot start (not found, no room for the socket, refused by the system) or has stopped, with its status and the wait before the next start. The bots' engines are told the connector is off, so they work without its tools. `projects_changed` follows every change, and each running engine of the project gets `connectors` again.

### ChatGPT sign-in

A user signs in with ChatGPT the way OpenAI documents for open-source local apps (<https://developers.openai.com/siwc/token-sharing-open-source>): the first sign-in registers Bate for this user (`client_id=dynamic_agent_client`, `agent_name_hint=Bate`, a host id made once for this Mac), later ones use the client id that came back. The request asks for `chatgpt.tokens.use.direct`; tokens without that scope are not kept. The code is traded with PKCE (`S256`), and the ID token must carry the request's nonce.

While signed in, every bot's model calls go to `https://api.openai.com/v1` through the Responses API with the access token; a saved model server is not used. An access token within 180 s of its end is renewed before it is handed to an engine, one renewal at a time (each renewal replaces the refresh token). When OpenAI says the refresh token will never work again, the tokens are forgotten, the waiting turns end with "sign in again in Settings", and `chatgpt_changed` is sent. Any other failure to renew leaves the sign-in as it is.

The registration is stored in `app.db`, the tokens in the Keychain (see Secrets); the tokens are removed from bug reports. Devices on the local network can neither read nor change the sign-in. `BATE_CHATGPT_AUTH` and `BATE_CHATGPT_API` name stand-ins for OpenAI's two servers (tests).

### The app

`window/bundle.sh` builds and signs `Bate.app`: `bate` (the window), `appd` and `agentcore` in `Contents/MacOS`, the service's job description `com.oursnever.bate.appd.plist` (`BundleProgram Contents/MacOS/appd`, `KeepAlive {SuccessfulExit: false}` as in `launch.rs`) in `Contents/Library/LaunchAgents`, and `Bate Helper.app` in `Contents/Helpers`. `SIGN_IDENTITY` picks the certificate: by default `Bate Dev` (`window/sign-dev.sh`) if the Mac has it, else ad hoc; a Developer ID Application identity goes there once there is one. With `BATE_NOTARIZE_PROFILE` (a `notarytool` keychain profile) the app is signed with the hardened runtime, notarised and stapled; that needs an Apple Developer ID, and the script never does it otherwise.

The window, started from an app, runs that app's `appd launch`. From `/Applications` or `~/Applications` that registers the service with `SMAppService.agent(plistName:)` once (an enabled one is left alone; an agent `appd launch` put in `~/Library/LaunchAgents` before makes way). If macOS wants the user's approval, `appd launch` opens System Settings > Login Items, prints `requires approval: …`, and the window's page shows the note (`service_note`) while the service is not there. macOS cannot start the login item of an app that changed (an update replaces the whole app) until it is registered again, yet goes on reporting it `enabled`, and a registration made soon after the change is lost too (measured on macOS 27: the first fails, the next one a few seconds later takes). So `appd launch` remembers the build it registered (the SHA-256 of `Contents/MacOS/appd`, in `run/login-item`), registers a new build again, and after each registration waits up to 10 s for launchd to start the service; if it did not, it unregisters, pauses 3 s and tries again, up to 6 times, and then fails with a reason, which the window shows as a note. An update ends by starting the new app's `appd launch` (its own process group; the job's plist has `AbandonProcessGroup`), so the service comes back without the user; a failure is in `logs/appd.log` and the next start of the window tries again. From anywhere else `appd launch` refuses. A service run from a build folder, or with `$APPD_HOME`, keeps the plist in `~/Library/LaunchAgents`.

A debug build reads `$BATE_BUNDLE` (the app to take itself to run from), `$BATE_LOGIN_ITEM_FAKE` (a folder that stands in for macOS's login items: the tests register nothing) and `$BATE_LOGIN_ITEM_ID` (another label and `<id>.plist`, for a trial of the real thing with an app of its own).

### Signatures

When `appd` is signed with a certificate (Developer ID, or `Bate Dev`), a window connects to the socket only if its code is validly signed with the same certificate (`SecCodeCheckValidity` on the peer's audit token, requirement `certificate leaf = H"<sha1>"`); another program is closed without a `hello`. The check is not a boundary against code running as the same user (the peer is looked at when it connects, not per message); per-message audit tokens (XPC) would be. The service's own connections (the LAN relay) pass. An ad-hoc or unsigned `appd` (a build folder) checks nothing beyond the user. A debug build reads the certificate's SHA-1 from `$BATE_SIGNER_LEAF` for tests.

### Updates

The service reads a static feed when it starts and then every 24 hours (and on `check_update`): `https://github.com/d41sys/bate/releases/latest/download/feed.json`, or `$BATE_UPDATE_FEED`. The feed is `{version, url, sha256, signature}`: `url` is a `bate-<tag>-macos-arm64.tar.gz` holding `Bate.app`, `sha256` its hash (64 lowercase hex digits), and `signature` an Ed25519 signature (base64) over the bytes `bate-update\n<version>\n<sha256>`. The version is signed too, so an old signed archive cannot be passed off as a new release.

The public key is compiled in from `BATE_UPDATE_PUBKEY` (base64) at build time. A build without one does not update (`state: "off"`). A debug build also reads the variable at run time, for tests; a release build never does. Nothing is staged unless all of these hold, and a failure is reported with its reason:

- the version is `a.b.c` and the signature matches it and the hash;
- the version is greater than the running one (a lower one is refused as older; the same one is just `current`);
- the download's SHA-256 is the feed's.

A release is downloaded to `updates/` under the service's home and only `Bate.app` is unpacked from it (`/usr/bin/tar`); it must hold `Contents/MacOS/appd` and `Contents/MacOS/agentcore`. Nothing is installed until `install_update`. Then the service waits until no bot has a turn, a prompt or a compaction in flight, so a running reply is never cut; it installs the release and exits with status 75. Run from a `Bate.app`, it copies the new app next to the old one (`ditto`) and exchanges the two atomically (`renamex_np`), so the signed app is replaced whole, never one program in it; the window keeps its old code until it is opened again. Run from anywhere else (a build folder), it copies `appd` and `agentcore` from the new app next to their targets, and renames both over the installed ones (next to the running `appd`, or `$BATE_UPDATE_INSTALL_DIR`). launchd restarts the service after any exit that is not 0 (`KeepAlive {SuccessfulExit: false}`), so the new `appd` starts; a service started by hand stays stopped. Engines do not outlive the service. A bot that begins a turn while an install waits keeps the install waiting.

`appd update keygen` prints a new key as `BATE_UPDATE_KEY=<private PKCS#8, base64>` and `BATE_UPDATE_PUBKEY=<public, base64>`. `appd update sign <archive> <version> [url]` reads `$BATE_UPDATE_KEY` and prints the feed; `url` defaults to the archive's place in the GitHub release `v<version>`.

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

### Desktop (Bate Helper)

Bots can see and control other apps through **Bate Helper**, a separate app (`Bate Helper.app`, at `~/Applications` unless `$APPD_HELPER` names its program: `…/Contents/MacOS/bate-helper`). It is the only program that holds the macOS permissions **Accessibility**, **Screen Recording** and **Full Disk Access**; the service and the window never need them. `helper/bundle.sh` makes the app from the `bate-helper` program (one script, so the installer can call it); an ad-hoc signature, the script's default, makes macOS ask for the permissions again after every rebuild.

The service listens on `helper.sock` in its run dir (mode 0600). On a bot's first desktop action, or when the user opens a System Settings pane from Settings, it starts the helper with `open -g -a <app> --args <socket>` (`$APPD_OPEN` names another `open`): through the system, so macOS gives the permissions to the helper and not to the service. The helper connects, and quits when the service goes. The service accepts a connection only from the user's own processes whose program file is the helper's, and the helper connects only to a process whose program file is named `appd` and, when the helper is signed with a certificate, that is signed with the same one (see Signatures); anything else is closed. In `Bate.app` the helper is `Contents/Helpers/Bate Helper.app`, and the service starts that one. Bots' engines cannot reach the socket (the folder limit denies it) or start apps.

Frames are one JSON object per line, at most 64 MiB, as in `proto::desktop`. Service → helper: `{id, action, …}` with `action` one of the six below, or `messages` (see Apple apps). Helper → service: `{"type":"status","accessibility","screenRecording","fullDiskAccess"}` (first, and at each change; the service sends `helper_changed`) and `{"type":"reply","id","output?","png?","error?"}`. A closed socket fails every waiting call (`Bate Helper quit before it answered.`); a call takes at most 30 s.

The engine's `desktop` tool is asked about as kind `desktop`, once for each exact action: the target is the whole action as the card shows it (`look at TextEdit`, `type into TextEdit: hello`, `press element 4 in TextEdit`; `list the apps and windows` for `apps`), never cut, so an `Always` for looking at an app is not one for typing into it, and what is typed is all on the card. The action is sent as a `desktop` event; the service checks its shape and answers with `desktop_result`. The helper finds an app by name, in any case, among the apps with a window on screen.

| `action` | Fields | Does |
|---|---|---|
| `apps` | none | Lists the apps with a window on screen and their windows (`window <id> "<title>" <w>x<h>`). Needs no permission (titles need Screen Recording). |
| `look` | `app`, `window?` | The window's accessibility tree, one element per line as `[n] Role "label" = "value"`, children indented; at most 500 elements and 10 s. A password field's value is not shown. The numbers hold until the next `look`. `window` is an id from `apps`; default the app's front window. Needs Accessibility. |
| `screenshot` | `app`, `window?` | A PNG of the window even where others cover it. The helper sends it as base64 `png`; the service saves it as `attachments/screenshot-<ms>.png` in the bot's folder and tells the bot the path. Needs Screen Recording. |
| `press` | `app`, `window?`, `element` | Presses the element (AX action `AXPress`). Needs a `look` of that window first. |
| `set_value` | `app`, `window?`, `element`, `value` | Sets the element's `AXValue` (at most 10 000 characters). |
| `type` | `app`, `window?`, `text`, `element?` | Types `text` (at most 10 000 characters): into the element, which is focused first (`AXFocused`), or into what has the focus. The keys are posted to the app's process (`CGEventPostToPid`), so the user's front app and pointer stay where they are. |

Without a permission, the call fails with a message that names it and where to allow it; `get_helper` and Settings show the same. Limits: windows that are minimised or on another Space are not listed; a few apps ignore keys posted to a background process; the model is given the tree and the picture's path, not the picture itself.

### Apple apps

Bate's engine has a tool for each of Messages, Mail, Notes, Reminders, Calendar and Contacts (`messages`, `mail`, `notes`, `reminders`, `calendar`, `contacts`), defined in `proto::apple`: a tool takes an `action` and some text fields (`id`, `query`, `to`, `title`, `body`, … per tool); other keys in `args` are ignored. The actions are:

| Tool | Reads (`apple_read`) | Sends or changes (`apple_change`) |
|---|---|---|
| `messages` | `read`, `search` (`to?`, `query`, `limit?`) | `send` (`to`, `text`) |
| `mail` | `search`, `read` | `draft`, `send` |
| `notes` | `list`, `read` | `create`, `edit`, `delete` |
| `reminders` | `list` | `create`, `edit`, `complete`, `delete` |
| `calendar` | `list` | `create`, `edit`, `delete` |
| `contacts` | `search` | `create`, `edit`, `delete` |

The engine checks the call (a known action; every field text of at most 100 000 bytes without a NUL; else the model is told so and nothing is asked) and asks a `question` first: title `<tool>: <action>`, `kind` `apple_read` or `apple_change`, `target` `<tool> <action>` and then each field that was given, one `name: value` per line, whole and as written. The card therefore shows the recipient and the text of a send exactly. Then it sends `apple` with the tool and the `args` it asked about, and waits for `apple_result`.

An `apple_change` is asked about every time: whatever its level (`always` is refused by `set_permission`, and treated as `ask` if one is found stored) and whatever was answered before, the user gets a card with the options `Approve` and `Deny`; `Always` on it is refused (`bad_request`). Only `never` answers without asking, with `Deny`. `apple_read` follows its level like any other kind and starts on `ask`.

The service runs the action, not the engine, because a limited bot's engine may not send Apple events or read the user's files (see Folder limit). It checks the call again with the same table, then runs the app's script with `/usr/bin/osascript` (at most 120 s; output cut at 64 KiB). Scripts are fixed text built into `appd`; what the model wrote reaches them only as `argv` (`-l AppleScript -e <script> -- <arguments>`), never as part of the source. Debug builds of `appd` take `BATE_OSASCRIPT` (another program) so tests record calls and touch no app.

Messages' history is read by the helper app, not by a script: the app's scripting cannot read old messages, and its history file `~/Library/Messages/chat.db` needs Full Disk Access, which the service must not hold (every program it starts would inherit it). `messages read` and `messages search` are `apple_read` calls: the service checks them (`limit` a number from 1 to 50, default 20; `query` needed by `search` and not allowed for `read`; `to` and `query` at most 200 characters on one line), then does not run a script but sends the helper `{id, action:"messages", query?, with?, limit}` (`to` is `with`; the `desktop` tool cannot send this action). The helper opens `chat.db` read-only (`SQLITE_OPEN_READONLY`) and answers with the newest matching messages, one per line, newest first: `<local time> [<conversation>] <me or sender>: <text>` (the conversation only when `to` is not given; further lines of a text are indented; at most 1000 characters of a text, `[attachment]` for a file). Text that Messages keeps only in `attributedBody` is decoded from it. `to` must name the conversation as Messages does (`+4915112345678`, an address), ignoring case, spaces and dashes; a search looks through the newest 20 000 messages. Without Full Disk Access the call fails with a message that says where to allow it (`get_helper` shows `fullDiskAccess: false` and Settings has a button that opens the pane; if Bate Helper is not listed there, the user adds it with `+`); with no `chat.db` it says there is no history. `messages send` goes to one person (`to`: phone number or address), not to a group. `mail` searches and reads the inbox only; `draft` leaves the message open in a Mail window, unsent. The first use of each app makes macOS ask the user to allow `appd` under Automation (and Contacts, Calendars and Reminders); a refusal comes back as an error that says where to allow it.

### Installed agents (ACP)

A bot can run on an agent installed on this Mac that speaks the [Agent Client Protocol](https://agentclientprotocol.com) over its standard input and output, in place of Bate's own engine: `set_agent {bot, command: ["opencode", "acp"]}`. `null` goes back. The agent uses its own login, model and tools. A bare program name is looked up on the service's `PATH`.

- `set_agent` stores the choice, forgets the bot's open conversation (the other engine could not reopen it; the transcript stays), and stops the bot's idle engine. `bots_changed` follows. During a turn it is refused with `busy`.
- The agent's login is in the user's home, so such a bot runs only with `wholeDisk` true. Otherwise its engine does not start (`engine_failed`, saying so).
- `agentcore acp` starts the agent process in the bot's folder, gives `session/new` and `session/load` the project's folder when the bot is in one, and translates: `start` is `session/new`, `open` is `session/load` (refused by an agent that cannot), `prompt` is `session/prompt`, `abort` is `session/cancel`. The agent's message and thought chunks become `text` and `thinking`; its tool calls become `tool_start` (`tool` is the agent's kind of tool; `args` holds `title`, `rawInput`, `locations`) and `tool_end`. Its plan becomes a `tool_start` and `tool_end` of the tool `plan` with the entries as `steps` (`completed` is `done`), so the window shows it as it shows the plan of Bate's own engine; an emptied plan is not sent.
- The agent's `session/request_permission` becomes a `question` with the options `Approve` and `Deny`. Its `kind` and `target` come from the agent's kind of tool (`read`, `search`: read; `edit`, `delete`, `move`: write; `execute`: run, with the command as target), so the bot's permission levels and grants apply; any other kind is asked of the user each time. The answer goes back as the agent's allow-once or reject-once option. What an agent does without asking, by its own settings, Bate does not see before it happens.
- The bot's enabled connectors are given to the agent when a conversation starts or is reopened (those over http only to an agent that takes them). A connector that signs in is given with the service's local address, as to Bate's own engine.
- Not offered to the agent: reading or writing files and running terminals through Bate (it does these itself). Refused by this engine: `steer`, `rewind`, `compact`. `set_model` is accepted and has no effect; choose the model in the agent. Memory, skills and messages between bots are Bate's own engine's and are not given to an agent.
- If the agent fails to open a conversation, the error names the agent's own ways to sign in.
- A turn that was aborted ends `aborted` however the agent ends it. A tool call the agent did not finish by the end of its turn gets a `tool_end` with `isError`. A tool's output is cut at 64 KiB.

### Other devices on the local network (LAN)

When enabled in Settings, the service serves Bate's web client over HTTPS on port 47822 (`APPD_LAN_PORT`) using a self-signed certificate generated once in `lan/cert.pem` with its SHA-256 fingerprint shown in the window. Its private key is kept in the Keychain, not in a file.

The same page is reachable from the user's own Tailscale network (no relay of Bate's): the listener is on every interface, so a device on the tailnet pairs and works as on the local network. The certificate also names this Mac's MagicDNS name and Tailscale addresses (from `tailscale status --json`, read when serving starts; `APPD_TAILSCALE` names another command) and is made again when these change, which changes its fingerprint. It is still self-signed: `tailscale cert` is not used. Local networks and Tailscale's 100.64.0.0/10 are alike treated as the user's own.

- Devices pair using a 6-digit code shown in Bate's window; pairing issues an `HttpOnly`, `Secure`, `SameSite=Strict` cookie containing a 32-byte hex token. The service stores only the SHA-256 hash of the token in `lan/devices.json`.
- All API requests require the `X-Bate: 1` header.
- Each paired web client opens a session via `POST /api/session`, which connects to Bate's Unix domain socket as an independent window connection. Commands are sent via `POST /api/send?session=…` and received via long-polling `GET /api/recv?session=…`.
- Removing a device in Settings terminates its sessions and revokes its cookie immediately.
- Files (`botfile://`) and surfaces open in the pane of Bate's native window only and are not served to LAN devices.

### Speech (the window, not the service)

Both parts run on this Mac with no network and send nothing to the service.

- **Dictation:** the **Dictate** button next to the message box focuses it and calls the shell's `start_dictation`, which sends `startDictation:` down the responder chain, the same action as the Edit menu's Start Dictation… (macOS adds that item to Edit itself). The system listens and types into the box; Bate asks for no microphone or speech permission. macOS decides where it runs: on the device on Apple silicon for the languages it supports, else it may send audio to Apple. Without Dictation turned on in System Settings, the call fails and the window shows why.
- **Read aloud:** each reply has **Read aloud** and, while it speaks, **Stop reading**. The page calls `speechSynthesis` with a voice marked `localService` (in the window, the system's voices). It reads the text as the page shows it: no Markdown marks, no link addresses, no code blocks. A reply that is still arriving is not read.
- **Web client on another device:** no **Dictate** button (the shell's call is not there, and a browser's speech recognition may send audio to its maker); **Read aloud** is shown when the browser has `speechSynthesis` and uses only its local voices.

### Bug report

`export_report` writes `reports/bate-report-<unixMs>.tar.gz` (mode 0600) under the service's home. It holds `logs/` (the service log and its older files) and `settings.json`: versions, and per bot its id, its routines (schedule, next and last run; no name, no prompt), and its connectors: the command, the number of arguments, the names of `env` and header entries, and a URL's scheme and host only. Conversations are not included.

Secrets are removed from the logs, and from `settings.json`, in three ways:

- Every stored connector value is replaced by `[removed]` where it is 4 characters or more: `env` values, header values and their parts, arguments with a `=` and their values, other arguments of 12 characters or more, a URL as a whole, its user and password, its query values and the pieces of its path of 12 characters or more. So are the service's `BATE_MODEL_API_KEY`, the key saved with `set_provider`, the tokens of a ChatGPT sign-in, and the tokens saved with `set_channel`.
- What follows one of these words and a `:` or `=` is removed, up to the next space or `&`, or up to the closing quote: `credential`, `signature`, `password`, `passwd`, `secret`, `token`, `pwd`, `sig`, and any word ending in `key` or `auth`. After `authorization`, `auth` and `cookie` the rest of the line goes. The word after `bearer`, and after a flag ending in one of the words (`--api-key value`), goes too.
- PEM blocks (`-----BEGIN` to the end of the `-----END` line) are removed.

Not recognised: a secret the service never stored that is printed with none of those words, and a stored value printed in another form (percent-encoded, base64).

### Channels (Telegram and Discord)

A bot can be written to from a chat app. The user makes a bot there (Telegram: BotFather; Discord: the developer portal) and gives Bate its token with `set_channel`. The service connects out from the Mac; nothing in between relays messages.

- **Telegram:** long polling, `GET <api>/bot<token>/getUpdates`, 10 updates at a time; replies with `sendMessage`. `<api>` is `https://api.telegram.org` (`BATE_TELEGRAM_API`). A message more than 2 minutes old is not run: Telegram keeps for a day what was written while the service was off.
- **Discord:** `GET <api>/gateway/bot`, then the gateway WebSocket (Identify with the direct-messages intent only, heartbeats; a broken connection starts a new session, without resume, so what was written meanwhile is lost; after a close the same token would get again, codes 4004 and 4010 to 4014, it is not tried again until the token is set anew); replies with `POST <api>/channels/<id>/messages`. `<api>` is `https://discord.com/api/v10` (`BATE_DISCORD_API`).
- Only one-to-one chats with text are read: Telegram private chats, Discord direct messages from people. Groups, servers and other bots are passed over; a paired chat that sends a photo or a file is told that only text is read.
- **Pairing:** a chat that is not paired gets a note on how to pair (unless 8 messages are already on their way out), and the bot hears nothing of it. The chat is let in by sending the code from `pair_channel`. A wrong 6-digit guess is answered and counted; a code is used up by the chat it lets in.
- A paired chat's message is sent to the bot as a prompt, as written. When the turn ends, the text the bot wrote since its last tool call goes back to the chat, in pieces of at most 1900 characters, a second apart. A turn that ends otherwise is answered `No reply: <why>.`
- While the bot is occupied, a message is answered that the bot is busy; it is not queued.
- `/run <name>` starts a routine that has `chat: true` (see Routines); any other message is a prompt.
- Questions and approvals are answered in Bate's window only; the chat gets a note that one is waiting.
- A new token for a channel keeps its paired chats (a Telegram chat is the same person for any bot; a Discord one is of no use to another bot and can be removed).
- Tokens are kept in the Keychain (see Secrets), are never sent to a window or an engine, are replaced in bug reports, and the four commands are refused to devices on the LAN.

### Storage

One SQLite database, `app.db`, in write-ahead mode with `synchronous=FULL`, written only by the service: bots, transcript, ledger, open questions, disk access, permission levels and grants, projects, rooms, sign-ins of connectors, routines, their past runs, connectors, and channels with their paired chats. When the service starts, a ledger entry still `sending` becomes uncertain and open questions are dropped, because engines do not outlive the service. A `groups` table left by an older version is dropped.

#### Secrets

Secrets are not in `app.db`; the service keeps each as a generic password in the macOS Keychain (login keychain; the service name is the launchd label, `com.oursnever.bate.appd`, with a suffix of its own for a service with a custom `APPD_HOME`, so two homes never share items): the model server's key (`provider`), the ChatGPT tokens (`chatgpt.access`, `chatgpt.refresh`), a connector's sign-in (`oauth:<bot>:<name>`, as JSON), a channel's token (`channel:<bot>:<kind>`) and the private key of the LAN certificate (`lan.key`, PEM). Their columns in `app.db` are empty. Removing a channel, connector sign-in, bot or the model server removes its items; an empty value removes one too.

An `app.db` from an earlier version is moved at start: each secret is written to the Keychain and read back, and only then blanked (`secure_delete` is on and the log is checkpointed, so its bytes are gone from the files). A secret the Keychain does not keep stays where it was, stays in use and is tried again at the next start. If the Keychain cannot be read, a secret still in `app.db` is used, and a secret still in `app.db` is the newer one. The same holds when a new value (a renewed refresh token) cannot be written: it is put in its `app.db` column instead, and moved at the next start. A `lan/key.pem` from an earlier version is moved the same way when the LAN page is turned on, and the file is removed.

`APPD_KEYCHAIN=<file>` makes the service use a keychain file of its own (made if missing) in place of the login keychain; the tests use it. Not passed on by `appd launch`. `window/sign-dev.sh` signs the programs so that the Keychain keeps allowing them after a rebuild.

Kept in files on purpose, and why: the key of a connector for the engine (`oauth.key`, `/mcp/<key>`) and a routine's webhook token are only good through the service's loopback port, and the window is shown the webhook URL; `lan/devices.json` has only hashes; `gate-chatgpt.json` is the output of the hand-run sign-in check (`appd gate chatgpt`), mode 0600, and is for deleting after the check.
