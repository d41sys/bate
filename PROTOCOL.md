# Engine protocol (v1)

The service (`appd`) runs one engine program per bot and talks to it over the program's standard input and output. Every engine (Bate's own `agentcore`, an ACP adapter, a test fake) implements this one interface. Types and helpers are in the `proto` crate.

## Which program runs

`$APPD_ENGINE` if set, else `agentcore` next to the service's executable, else `agentcore` on `PATH` (`proto::engine_program`). A relative `APPD_ENGINE` path is resolved against the service's working directory, not the bot's folder. Set `APPD_ENGINE` to develop, test or swap an engine without changing the service.

The service starts the program with the bot's folder as its working directory (`proto::Engine::spawn`). The engine's standard error goes to the service log.

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
| `rewind` | `entry`, `fork` | Go back to `entry`: in place, or as a new conversation when `fork` is true (the engine sends the new `session`). |
| `compact` | none | Summarise the working context now. |
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
| `question` | `question`, `title`, `options` | The engine waits until the service sends `answer`. Every action that changes something (a file write, a command) is asked this way. |
| `turn_end` | `reason`, `error?` | The turn ended: `done`, `aborted` or `error`. |

A killed engine sends nothing more.

## Window ⇄ service (v1)

`appd` is the per-user service (launchd label `com.oursnever.bate.appd`). Its data lives in `~/Library/Application Support/Bate/`, or in `$APPD_HOME`; a custom home gets the label `com.oursnever.bate.appd.<hash>` so a test never touches the real agent. `appd launch` registers and starts it; `appd unregister` stops and removes it. A second `appd` for the same home exits at once (`run/appd.lock`).

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

Match replies by `id`, never by order. Codes: `hello_required`, `parse`, `unknown_command`, `bad_request`, `no_such_bot`, `no_such_question`, `no_such_routine`, `invalid_routine`, `no_such_connector`, `invalid_connector`, `engine_failed`, `engine_error`, `engine_stopped`, `internal`.

| Command | Fields | `data` |
|---|---|---|
| `hello` | `protocol` | `{protocol}` |
| `list_bots` | none | `{bots:[Bot]}` |
| `create_bot` | `name` (1 to 80 characters) | `{bot: Bot}` |
| `open_bot` | `bot` | `{bot: Bot, history:[Entry], questions:[question event]}` |
| `prompt` | `bot`, `nonce`, `text` | `{}` once the engine admitted it; `{duplicate:true}` or `{duplicate:true, uncertain:true}` for a known nonce (see Prompt ledger) |
| `answer` | `bot`, `question`, `choice` | `{}` once the engine took it; `no_such_question` if that question is not open |
| `steer` | `bot`, `text` | `{}` |
| `abort`, `compact` | `bot` | `{}` |
| `set_model` | `bot`, `model`, `effort?` | `{}` |
| `rewind` | `bot`, `entry`, `fork` | `{}` |
| `list_routines` | `bot` | `{routines:[Routine]}` |
| `set_routine` | `bot`, `routine` | `{routine}`. No `routine.id`: create; with one: replace |
| `delete_routine` | `bot`, `routineId` | `{}` |
| `list_connectors` | `bot` | `{connectors:[Connector]}` |
| `set_connector` | `bot`, `connector` | `{connector}`. Creates or replaces by `name` |
| `delete_connector` | `bot`, `name` | `{}` |

`Bot` is `{id, name, createdAt, running, busy}`: `running` while the bot has an engine, `busy` between `turn_start` and `turn_end`. Every engine operation starts the bot's engine if it has none; the service then sends `start`, or `open` with the bot's last `session`. If the engine refuses `open`, the service forgets that session and sends `start`.

### Events (service → every window that said hello)

| Frame | Meaning |
|---|---|
| `{"type":"transcript","bot","seq","at","entry"}` | An entry was appended to the bot's transcript. |
| `{"type":"bots_changed"}` | Re-run `list_bots`. |
| `{"type":"question_closed","bot","question"}` | The engine took an answer to the question. |
| `{"type":"engine_stopped","bot"}` | The bot's engine exited. Its open questions are dropped. |
| `{"type":"routines_changed","bot"}` | Re-run `list_routines`. |
| `{"type":"connectors_changed","bot"}` | Re-run `list_connectors`. |

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

### Questions

An engine `question` is stored until the engine takes an `answer` to it (an answer the engine refuses leaves it open) or until the engine stops. `open_bot` lists the open ones, so a window that reconnects still sees them.

### Routines

Same shapes as `proto::routines`: a routine is a prompt sent on a daily, weekly, monthly or yearly schedule at a wall time in an IANA zone, every `interval` periods counted from `anchor`. The service sets `id`, `anchor`, `nextRunMs` and `lastRun`. `name` is 1 to 80 characters, `prompt` 1 to 8000, and a bot has at most 50 routines.

At a slot the service sends `"[Routine: <name>]\n<prompt>"` with nonce `routine-<id>-<slotMs>`, through the ledger, so a crash cannot send it twice. A slot missed while the service was down runs once if it is at most 6 h old; an older one is recorded as `missed`. A wall time that does not exist (clocks jump forward) runs when the gap ends; one that happens twice runs at the earlier instant.

### Connectors

Stored per bot (`proto::connectors`): `name` is 1 to 40 characters of `a-z`, `0-9`, `_`, `-`; `transport` is `stdio` (`command`, `args`, `env`) or `http` (`url` starting with `http://` or `https://`, `headers`). Values are stored as plain text. Engines load them in #9.

### Storage

One SQLite database, `app.db`, in write-ahead mode with `synchronous=FULL`, written only by the service: bots, transcript, ledger, open questions, routines and connectors. When the service starts, a ledger entry still `sending` becomes uncertain and open questions are dropped, because engines do not outlive the service.
