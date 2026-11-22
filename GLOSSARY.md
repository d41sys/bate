# Bate

A personal bot app for the Mac: a host where a person and bots work on projects together.

## Language

### People and bots

**Bot**:
A named assistant with its own instructions, memory, permissions and conversations. It is in no project or in one.
_Avoid_: agent, assistant

**Paused**:
A bot the person stopped until they resume it: its turn is stopped, its routines skip, other bots' messages wait, and it takes no prompt from the person.
_Avoid_: disabled, muted, stopped

**Token limit**:
What a bot may use per day and per month, in tokens. It stops unattended work (routines, other bots' messages, channels) or everything.
_Avoid_: budget, quota, spend cap

**Conversation**:
One exchange between the person and a bot: the prompts, the replies and what the bot did, in order. A bot has many.
_Avoid_: chat, thread, session

**Engine**:
The program that runs a bot's turns: Bate's own, or an installed agent.
_Avoid_: harness, driver, runtime

**Installed agent**:
Another maker's agent program used as a bot's engine.
_Avoid_: external agent, CLI

**Role**:
A ready-made definition of a bot that a project can add as one of its bots.
_Avoid_: persona, template

### Work

**Project**:
One piece of work: a folder that a person and the project's bots share, with shared instructions and memory. It is not part of a bot; a bot is in at most one project.
_Avoid_: workspace, group, team

**Room**:
A project's one conversation: the person writes to its bots, by `@name` or to its lead, and their replies come back in one timeline.
_Avoid_: channel, chat

**Workplace**:
An installed feature that equips a project for one kind of work, such as coding or reverse engineering.
_Avoid_: plugin, extension, feature, app

**Surface**:
Something in the window that the person and the bots both see and act on: a table, a tree or a code view that a workplace's program fills.
_Avoid_: pane, panel, view, block

**Connector**:
A tool server that a bot can call.
_Avoid_: integration, plugin

**Version**:
What a file was before a change, kept so it can be put back: the text a bot's `write` or `edit` replaced, or a note before it was replaced or deleted.
_Avoid_: backup, snapshot, revision

**Skill**:
A folder of instructions that a bot reads when a task calls for it.

**Bot file**:
One file that holds a bot's instructions, skills, routines, notes and connectors, to give to someone or to move to another Mac.
_Avoid_: backup, archive, template

**Routine**:
A prompt that a bot runs on a schedule or when something triggers it.
_Avoid_: job, automation, cron

**Precheck**:
A command a routine runs before the bot, with no model, under the bot's own limits. If it ends well, the bot runs with its output; if not, the run is skipped. Saving the routine is the person's approval of that exact command.
_Avoid_: hook, guard

### The app

**Channel**:
A messaging service outside Bate, such as Telegram or Discord, through which people write to one bot.

**Device**:
Another computer or phone, paired with this Mac, that uses Bate through its page.

**Quick Ask**:
A small floating window that a global shortcut opens over any app, to ask one bot one question; the answer shows there, and it can go on in the bot's own window.
_Avoid_: popup, launcher
