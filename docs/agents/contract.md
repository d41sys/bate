# Rules for a change

What every change to this repo follows, whoever makes it. `PROTOCOL.md` says what the service does; this file says how a change is made and checked.

## Where to work

- One git worktree and one branch per issue, made from `origin/main`. The main checkout is not edited.
- A build folder of its own per worktree (`CARGO_TARGET_DIR`), removed after the merge.
- The window crate needs the page built first: `cd window/ui && npm ci && npm run build`.

## What a change must hold to

- The issue is the specification: build everything its "What" names. A part that cannot be built is said, not left out.
- The smallest complete change: reuse what exists, no option or abstraction nobody asked for.
- The core thread (`Core::run` in `appd/src/core.rs`) never waits for a command, a download, a connector, a model or a person. Such work runs on a thread of its own and reports back as a message.
- A device on the network gains no power by accident: every new command and every new field that changes what the service does by itself comes with a decision, written in the pull request, on whether a device may use it.
- Nothing is allowed that the person was not shown. Text from a bot, a connector or a page is data: bounded, and marked as data where it goes into a prompt.
- A secret is in no frame, row, log or prompt.
- A per-bot table is in `BOT_TABLES` (`appd/src/store.rs`), so that deleting a bot deletes its rows.
- UI follows `docs/design-system.md`: tokens only, components in `style.css` and the doc's table, no inline style, no new dependency.
- Words follow `GLOSSARY.md`. Public text names no other product.

## Tests

- A test catches a bug a person or a client of the service would see: service behaviour through the socket (`appd/tests/service.rs`), engine behaviour with the replaying model server (`agentcore/tests/engine.rs`), UI logic in `window/ui`.
- No dependence on timing: wait for stored state, not for a live frame that may have been sent already; wait for a whole line of a file, not for the file; a file gate, not a sleep.
- A temporary folder lives as long as what uses it.
- No network. No real secret: a test that needs one builds a made-up word when it runs.
- A service started by hand has `APPD_HOME` and `APPD_KEYCHAIN` in a temporary folder, and is stopped by its process id.

## The gate

All four pass before a push:

```
cd window/ui && npm run build && npm test
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

## The pull request

- Three sections: **Summary** (ending `Closes #N.`), **Evidence** (what was run and seen; for UI, pictures in light and dark, wide and narrow, and whether they are of the real window), **Merge Danger** (what could break, what was not checked).
- Pictures of the page: build it, then `npm run shots -- <folder> [screen ...]` in `window/ui` saves each screen (`window/ui/standin/screens.mjs`) in light and dark, 1200 and 390 wide, with Google Chrome headless; `npm run standin` serves the same page, each screen at `/<name>`. Both run the built page against the stand-in's made-up answers (`window/ui/standin/answers.mjs`), so such pictures are of a stand-in, not of the real window: say so in the pull request. A command the page learns to send needs its answer there, or `npm test` fails.
- Rebased on `origin/main`, with the gate run again after the rebase.
- Merged only when both checks of the pull request have run and passed, and after its diff was read.

## When a review finds the same thing twice

Move the fix up: first into the code so the mistake cannot be made, else into a test or a lint that fails, else into this file. A reviewer's memory is the last place for a rule.
