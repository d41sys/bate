# Architecture

![Bate architecture: the window, other devices and chat apps talk to the appd background service, which keeps state in app.db and the Keychain, runs one sandboxed agentcore engine per bot and drives Chrome, Bate Helper and Apple apps, while the engine calls the model, MCP connectors and an optional installed agent.](diagrams/bate-architecture.svg)

The window, other devices and chat apps only talk to `appd`; it owns the state and the permission questions. Each bot's `agentcore` engine keeps its state in the bot's folder, works in its project's folder when it has one, and asks `appd` before every tool call.
