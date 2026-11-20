# Bate

A personal bot app for the Mac: a host where a person and bots work on projects together.

## Language

### People and bots

**Bot**:
A named assistant with its own instructions, memory, permissions and conversation. It is in no project or in one.
_Avoid_: agent, assistant

**Conversation**:
A bot's one exchange with the person: the prompts, the replies and what the bot did, in order.
_Avoid_: chat, thread, session

**Engine**:
The program that runs a bot's turns: Bate's own, or an installed agent.
_Avoid_: harness, driver, runtime

**Installed agent**:
Another maker's agent program used as a bot's engine.
_Avoid_: external agent, CLI

**Group**:
Bots that may message each other, one of them the lead, whatever projects they are in. Not a place: it has no folder and no room.
_Avoid_: team

**Role**:
A ready-made definition of a bot that a project can add as one of its bots.
_Avoid_: persona, template

### Work

**Project**:
One piece of work: a folder that a person and the project's bots share, with shared instructions and memory. It is not part of a bot; a bot is in at most one project.
_Avoid_: workspace

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

**Skill**:
A folder of instructions that a bot reads when a task calls for it.

**Routine**:
A prompt that a bot runs on a schedule or when something triggers it.
_Avoid_: job, automation, cron

### The app

**Channel**:
A messaging service outside Bate, such as Telegram or Discord, through which people write to one bot.

**Device**:
Another computer or phone, paired with this Mac, that uses Bate through its page.
