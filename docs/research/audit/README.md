Of the 123 "Done when" clauses in issues #1 to #39, a person using the app gets 88; 6 exist only on the service's socket, 11 are not delivered, and 18 wait for a hand check on a real Mac.

# Closed issues against their own "Done when"

Audited 2026-11-18, from the code, clause by clause, from the seat of a person using the installed app. Nothing was run. A clause counts as met only when the window reaches it (or, for engine-only clauses, when the engine code and a test cover it).

| Issues | File | Clauses | Met | Socket only | Not met | Needs a hand check |
|---|---|---|---|---|---|---|
| #1 to #13 | [issues-01-13.md](issues-01-13.md) | 52 | 47 | 1 | 3 | 1 |
| #14 to #26 | [issues-14-26.md](issues-14-26.md) | 38 | 27 | 2 | 6 | 3 |
| #27 to #39 | [issues-27-39.md](issues-27-39.md) | 33 | 14 | 3 | 2 | 14 |
| **Total** | | **123** | **88** | **6** | **11** | **18** |

## Closed without being delivered, now reopened

| Issue | The clause | What is true | Evidence |
|---|---|---|---|
| #22 Memory | "a panel lists them" | No memory panel. | The window never calls `list_memory`, `read_memory`, `delete_memory`. |
| #23 Skills | "a skill folder can be installed for a bot" | Socket only. | The window never calls `list_skills`, `install_skill`, `delete_skill`. |
| #31 Schedules | "each routine shows its past runs" | Only the newest run is shown. | `window/ui/src/Routines.tsx:50`; `list_routine_runs` is never called. |
| #33 Bot teams | "a bot can message another bot ... a group of bots can work under a lead" | Neither is possible in the app. | No window call to `set_group`; the message tool is offered only to bots with group peers (`agentcore/src/turn.rs:91`). |
| #14 Fork | "forked" | Fork cuts the chat like Edit; the original cannot be opened; the tooltip says otherwise. | `window/ui/src/Chat.tsx:470`; `Settles::Rewind` in `appd/src/core.rs`. |
| #9 Compaction | "and on request" | Socket only; the window size is fixed at 32,000 tokens. | The window never calls `compact`; `agentcore/src/main.rs:427`. |

How this happened: the closing comments of #22 and #23 left the screen to #12 ("comes with the window"), and #12's list of screens did not include them. The issues were closed on service tests alone.

## Closed by a decision, not built

Left closed. They are listed so the decision can be looked at again.

| Issue | The clause | What happened |
|---|---|---|
| #17 Claude sign-in | "A Claude subscription can be signed in to from settings", with a notice | Closed as won't-fix on 2026-11-17: outside Anthropic's terms. The route offered instead is Claude's own program as an installed agent (#27). |
| #1 Sign-in gate, Claude half | "The same with a Claude sign-in token" | Dropped on 2026-11-17 for the same reason. |
| #18 Usage | "Usage against the subscription's limits is shown where the provider reports it" | A label and a link only. The closing comment says OpenAI's plan route reports no figure; that claim was not verified here. The Codex sign-in route does report usage (see `../components/01-models-and-sign-in.md`). |
| #25 Bot pages | "no network unless allowed" | No network at all; there is no way to allow it. Stated as a limit in the closing comment. |

## Built, never tried for real

- #38 Updates: the repo has no release and no tag, so the default feed has never answered.
- #13: a 10,000-message chat scrolling smoothly has only the author's own measurement.
- #15: the menu-bar icon is not named as checked in any closing comment.
- #21, #28, #29, #34, #36, #37: open for their hand checks, as before.

## Limits of this audit

- Read from code and tests; no app, build or test was run.
- "Met" says the window reaches the feature, not that it works well; how it compares with other apps is in `../components/`.
- Each part file lists what its author could not verify.
