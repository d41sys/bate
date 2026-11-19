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

In the Projects tab, give the whole paths of one or more role files to a project. Bate shows what each will create: the name, model, skills and permission levels. A level that lets the bot do more than a new bot may (`always` for anything but `read`) is listed, and is applied only if you agree; otherwise that kind stays on `ask`.

A file that is malformed, or names a skill folder that is missing or holds another skill, is refused with the reason, and no bot is created. The protocol is in `PROTOCOL.md` (`read_roles`, `add_roles`).
