# Engine protocol (v1)

The service (`appd`) runs one engine program per bot and talks to it over the program's standard input and output. Every engine (Bate's own `agentcore`, an ACP adapter, a test fake) implements this one interface. Types and helpers are in the `proto` crate.

## Which program runs

`$APPD_ENGINE` if set, else `agentcore` next to the service's executable (`proto::engine_program`). Set `APPD_ENGINE` to develop, test or swap an engine without changing the service.

The service starts the program with the bot's folder as its working directory (`proto::Engine::spawn`). The engine's standard error goes to the service log.

## Frames

- One JSON object per line, UTF-8, `\n`-terminated.
- Service → engine lines are capped at `proto::MAX_OP_FRAME` (1 MiB). Engine → service lines at `proto::MAX_EVENT_FRAME` (64 MiB), because tool output is forwarded whole.
- A line over the cap ends the connection. A line that is not a JSON object, or whose `type` is not in the lists below, is refused and the next line is read normally.
- Field names are camelCase. `type` values are snake_case.

## Start-up

The engine's first line is `{"type":"ready","protocol":1}`. The service sends nothing before it.

## Operations (service → engine)

Each request is `{"id":"…","type":"<op>",…fields}`. `id` is a string chosen by the service. The engine answers each request exactly once with `{"type":"response","id":"…"}`, or with `"error":"text"` added when it fails. Match responses by `id`, never by order.

| `type` | Fields | Effect |
|---|---|---|
| `start` | `bot`, `workdir` | Begin a new conversation. The engine sends `session`. |
| `open` | `bot`, `workdir`, `session` | Reopen an earlier conversation. The engine sends `session`. |
| `prompt` | `nonce`, `text` | Send a user message. `nonce` is the prompt ledger's key and comes back in `turn_start`. |
| `steer` | `text` | Add a message to the turn that is running. |
| `abort` | none | End the running turn now. It ends with `turn_end` reason `aborted`. |
| `answer` | `question`, `choice` | Answer an open `question`. `choice` is one of its `options`. |
| `set_model` | `model`, `effort?` | Model and thinking effort for the following model calls. |
| `rewind` | `entry`, `fork` | Go back to `entry`: in place, or as a new conversation when `fork` is true (the engine sends the new `session`). |
| `compact` | none | Summarise the working context now. |
| `stop` | none | Finish and exit. The engine answers, then closes its output and exits with status 0. |

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

A killed engine sends nothing more. The service marks that bot's unfinished tool calls as Interrupted.

## Window ⇄ service

The window speaks to the service over the service's socket (#3). The service forwards each engine event to windows unchanged, labelled with its bot. The window's per-bot commands are these operations with a `bot` field added.
