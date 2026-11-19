---
status: accepted
---

# Workplaces are separate programs that speak Bate's protocol

Bate is a host: it owns projects, bots, their teamwork and the surfaces a person and the bots share. A feature for one kind of work (coding, research, reverse engineering) is a **workplace**: a package a user installs only if they want it, with its own program that Bate's service starts in a sandbox, one per project. The program offers tools to every bot in the project over MCP and fills surfaces that the window draws natively. We chose this so that a user carries only the features they use, anyone can write a workplace in any language, and a workplace cannot read Bate's own memory or Keychain.

## Considered options

- **Compile every feature into Bate and switch it on per project.** Rejected: every user carries every feature, and nobody outside this repository can add one.
- **Load downloaded code into Bate's own process** (a script runtime, or native libraries). Rejected: a notarised Mac app keeps library validation on, Rust has no stable format for loadable modules, and such code would hold Bate's permissions. The large features (a disassembler, a virtual machine, a speech model) are separate programs anyway.
- **A generic tool server per bot, with a web page in a frame.** Rejected: each bot would run its own copy, so the bots and the person would not share one set of data, and the feature could take no part in the window's layout.

## Consequences

- The service owns a workplace's program; a bot's engine does not start its own copy. Engines, including installed agents used through ACP, reach the program through a local address the service hands out.
- Bate owns a fixed set of surface kinds. A workplace names a kind and sends content. A web page in a sandboxed frame is the fallback for anything the kinds cannot draw.
- Bate's own workplaces are built and shipped the same way as anyone's.
- A workplace may describe limits, such as the hosts a security test may touch. The service and its sandbox enforce them; a workplace is not trusted to limit itself.

Prior art read before deciding: Pi packages and Pi's plugin primer, omp plugins, the bundled plugins of OpenClaw and Hermes, and Agent Plugins as nanobot loads them.
