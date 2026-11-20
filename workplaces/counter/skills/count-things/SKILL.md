---
name: count-things
description: Use when the user wants a running total shared by the whole project: call the counter's `tick` tool.
---
The project has a counter that all its bots share. To add one, call the tool `counter__tick`; it returns the new total. Never keep your own count: another bot may have ticked since.
