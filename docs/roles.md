# Role files

A role is a ready-made bot that a project can add. It is a Markdown file: a front matter between two `---` lines, then the bot's instructions.

```
---
name: Reviewer
model: gpt-5
effort: high
skills: code-review, pdf-forms
write: never
run: always
---
Review every change before it is merged. Point out bugs first, style last.
```

## Keys

One `key: value` per line. Empty lines and lines starting with `#` are skipped. A key may appear only once.

| Key | Value |
|---|---|
| `name` | Required. The bot's name, 1 to 80 characters. |
| `model` | Optional. The model the bot uses, as in the model menu. |
| `effort` | Optional, only with `model`: `minimal`, `low`, `medium` or `high`. |
| `skills` | Optional. Skill names separated by commas. Each is a skill folder (with its `SKILL.md`) in the same folder as the role file, named after the skill. |
| `read`, `write`, `run`, `connector`, `desktop`, `apple_read`, `apple_change` | Optional. The permission level for that kind of action: `always`, `ask` or `never`. A kind left out keeps a new bot's level: `read` on `always`, the rest on `ask`. `apple_change` cannot be `always`. |

The rest of the file, at most 8000 characters, is the bot's instructions. The file is at most 64 KiB.

## Adding roles to a project

On a project's Bots page, give the whole paths of one or more role files to the project. Bate shows what each will create: the name, model, skills and permission levels. A level that lets the bot do more than a new bot may (`always` for anything but `read`) is listed, and is applied only if you agree; otherwise that kind stays on `ask`.

A file that is malformed, or names a skill folder that is missing or holds another skill, is refused with the reason, and no bot is created. The protocol is in `PROTOCOL.md` (`read_roles`, `add_roles`).

## Ready-made roles

Bate ships one role, in this same format: **Lead** (`roles/lead.md` in the source). Its instructions tell a bot to answer a short question itself, hand anything more to the fitting bot of the project with an acceptance command in `check`, read what the check showed, say plainly when it failed after the worker's second try, and report to the person what was done, by whom, and what the check showed. It sets no level, so it has a new bot's. The project's Bots page offers it beside adding a role from a file (**Add a Lead**). In a project that has no lead of its own the new bot becomes its lead; a project whose lead was chosen keeps it.
