# Design system

How Bate's window looks and how its screens are built. One set of tokens, one set of components, and rules for layout and accessibility. It answers #138 and takes its inputs from the UX research (`docs/research/ux/`).

## The direction

The owner chose a chat-app look: a bot is a contact, a conversation is messages.

- **Neutral.** White, greys and near-black in light; the same reversed in dark. One ink colour marks what stands out: the person's message, the main button.
- **Blue is rare.** It is for links, the focus ring and nothing else. A bot's own colour shows on its avatar only.
- **Round.** Messages are bubbles, buttons and the composer are pills, cards have soft corners.
- **No bar on top.** Who you are talking to floats above the page as a chip, with the tabs beside it.
- **The system's font**, 14 px on a 20 px line.

| Light | Dark |
|---|---|
| ![The target screen in light](design-system/target-light.webp) | ![The target screen in dark](design-system/target-dark.webp) |

The pictures are `docs/design-system/shell.html`: a static page drawn with the app's own stylesheet. Open it in a browser to resize it or to see it in dark.

Three other directions were drawn on the same screen and not chosen: one close to the Mac's own controls, one made for reading with a serif face, one dense with monospace labels.

## Rules

1. **Tokens only.** A screen or a component names no colour, size, font, radius or duration. It uses a token from `window/ui/src/tokens.css`. A value that is missing becomes a new token first.
2. **Elements first.** A plain `<button>`, `<input>`, `<select>`, `<textarea>`, `<fieldset>` or `<h2>` already looks right. A class is for a variant (`primary`, `danger`, `ghost`, `small`, `round`) or a component that has no element.
3. **No inline `style=` for looks.** Inline style is for values the code computes: a width being dragged, a bot's colour passed as `--tint`.
4. **Light and dark are the same components.** Dark changes token values under `prefers-color-scheme`, never a component.
5. **No new dependency.** See "Build or adopt".

## Tokens

All in `window/ui/src/tokens.css`.

| Family | Tokens | Use |
|---|---|---|
| Surface | `--bg`, `--bg-side`, `--bg-raised`, `--bg-soft` | The page, the sidebar, what lies above the page (cards, fields, dialogs), a quiet fill (a bot's message, code) |
| State fill | `--bg-hover`, `--bg-current` | Under the pointer; the chosen row or tab |
| Text | `--fg`, `--fg-muted` | Text; secondary text and placeholders |
| Line | `--border`, `--border-strong`, `--border-field` | A divider; the edge of a button or card; the edge of a field (3:1) |
| Ink | `--ink`, `--on-ink` | The main button and the person's message, and the text on them |
| Meaning | `--accent`, `--danger`, `--warn`, `--warn-soft`, `--mark` | Links and focus; a destructive action or an error; a warning and its fill; a search hit |
| Depth | `--shadow-1`, `--shadow-2`, `--scrim`, `--glass` | A card; a dialog or sheet; behind a dialog; behind the floating header |
| Type | `--font-ui`, `--font-mono`, `--size-small` 12, `--size-ui` 14, `--size-title` 17, `--size-head` 22, `--leading` 20, `--weight-strong` 600 | |
| Space | `--space-1` to `--space-5`: 4, 8, 12, 16, 24 | A 4 px grid |
| Shape | `--radius-s` 6, `--radius-m` 10, `--radius-l` 14, `--radius-bubble` 18, `--radius-pill` | Code and small things; fields and rows; cards and dialogs; messages; buttons |
| Size | `--control-h` 32, `--control-h-small` 24, `--row-h` 44, `--avatar` 28, `--side-w` 280, `--thread-w` 720, `--popover-w` 300, `--chip-w` 180, `--tip-w` 240 | |
| Motion | `--fast` 100 ms, `--ease` | Hover, press, showing a message's actions |

## Components

All in `window/ui/src/style.css`. "Built" means it is there now; a number is the issue that builds it.

| Component | How to write it | States | Built |
|---|---|---|---|
| Button | `<button>`; `primary`, `danger`, `ghost`, `small`, `round` (one sign, needs `aria-label`). A form's submit button is primary by itself | hover, pressed, disabled, focus | yes |
| Text field, select | `<input>`, `<textarea>`, `<select>`; in a form, `.fields` with one `<label>` per control; `.fields.stack` puts each label over its control, as wide as the column | placeholder, disabled, focus | yes |
| Form row with hint, error and "Saved" | | | #155 |
| Switch | | on, off, disabled | #155 |
| Tabs | `role="tablist"` with `role="tab"` buttons (`id="tab-…"`, `aria-controls="tabpanel"`); only the chosen tab is in the Tab order. Left and right arrows choose the neighbour and wrap, Home and End the first and last. What the tab shows is inside `<div role="tabpanel" id="tabpanel" aria-labelledby="tab-…">`, which adds no box (`display: contents`) | chosen, hover, focus | yes |
| List row | `.row` with an `.avatar` and a `.label` (a name and a `<small>` line under it); `current` for the chosen one. A row to tick is `<label class="row">` with a checkbox before the avatar; one that cannot be ticked is dimmed and its `<small>` says why | hover, chosen, disabled, focus | yes |
| Badge | `.badge` at the end of a `.row`: a dot and one word. Plain (warn) for what needs the person: "Needs you". `.badge.quiet` for what does not: "New reply". A working bot has no badge: its `.label` has a `<small>Working…</small>` line | | yes |
| Sidebar | `<nav class="sidebar">`: an `<h2>` for each section (Projects, Bots; the Bots heading sits in a `.heading` with its `+` beside it, a `round ghost` button), a `<ul>` of rows, a nested `<ul>` for a project's bots, `.row.plain` for a row with no avatar, and `<ul class="foot">` for the app's own pages at the bottom | hover, chosen (`current`, with `aria-current`), focus | yes |
| Rows of settings or items | `<ul class="rows">` | | yes |
| Avatar | `.avatar` with `--tint` set to the bot's colour | with a sign, with a letter, without a colour | yes |
| Message | `.item.user` or `.item.reply` with a `.bubble`; `.actions` under it; in the Room a `<small class="from">` above it says who wrote it (avatar, name, "to" whom, time) | past (greyed), marked (a search hit) | yes |
| Step of a turn | `.item.tool` with `<details>` | running, done, failed | yes |
| Card | `.card`. `.question` is the approval card of a risky action: the title, the exact command or file in a `<pre>` (it scrolls when long), `In <code>folder</code>`, then `.actions` with **Allow once** (`primary`), **Always** and **Deny** (`danger`, set apart at the end), a `.note` saying what Always covers and that the bot asks again for anything else, and a `.note` that a message typed instead denies it. Nothing in it has focus when it appears. Esc denies; allowing by key is the chord ⌘Enter, never Enter alone | the first card shows the keys (`<kbd>`); Always is absent for an action that always asks | yes |
| Composer | `<form class="composer">`: round buttons, one `<textarea rows="1">` that grows, the model chip (a popover) beside it. The Room's has no chip for a model: typing `@` opens a `.popover.start` listing the project's bots as `.row.plain` options, and the pick becomes a `.mention` (an avatar, `@name` and a remove button, like an attached file) in front of the words; Enter picks while the list is open, Esc closes it, Backspace on an empty box removes the pick. While the bot works, `.hints` under it says what Queue and Steer do, and Esc stops it (Esc denies instead while an approval card waits) | empty, typing, bot working, focus | yes |
| Note, error line | `.note`, `.error` (with `role="alert"`) | | yes |
| Banner | `.banner`, `.banner.error`; `.banner.actions` when it offers a button (`<button class="small">`), as the line "Daisy waits for your answer" does | | yes |
| Starter card | `<ul class="starters">` of `<li><button type="button" aria-pressed>`, each with an `.avatar` and a `.label` (a name and a `<small>` line). A card fills a form; it is not a link | hover, chosen (`aria-pressed`), focus | yes |
| Empty state | `.empty`: one sentence, then one action | | yes; per screen with its issue |
| Dialog | `<dialog class="sheet">` opened with `showModal()` and holding one `<form class="fields stack">`: the dialog is the scrim, the form the panel, `.actions` stay at the foot while it scrolls. Focus goes to the first field, stays inside, and returns to the button that opened it; Esc or a click on the scrim closes it | scrolling | yes (Projects); the palette is its own overlay, #159 |
| Tooltip | `data-tip="…"` on a control; the text is shown above it for a pointer or the keyboard, and not on a touch screen, so it never holds the only copy of something a button must say | hover, focus | yes |
| Button with a hint key | `<button>Stop <kbd>Esc</kbd></button>`: the key that does the same, hidden under 640 px | | yes |
| Popover, select with search | `.menu` around a chip (`<button class="small chip" aria-expanded>`) and a `.popover` (`role="dialog"`) holding a search `<input>`, a `<ul role="listbox">` of `.row.plain` buttons and other controls. A click outside or Esc closes it; Esc is taken there and does not reach the page | closed, open, chosen row, no match | yes |
| Conversation switcher | `<div class="conversations">` above the chat: a `.menu` with a chip (the conversation's name) whose `.popover.below` lists the bot's conversations as `.row.plain` buttons with a date under the name (or "Working…"), `.actions` of small ghost buttons beside each (Pin, Rename, Archive), and a "Show archived" checkbox; a "New conversation" button after it. A rename is a one-field form in the row | closed, open, chosen row, working, pinned, archived, renaming | yes |
| Folder picker | A text box for the whole path with a `Choose folder…` (or `Choose file…`) button beside it, in a `.field` (a label over a `.find`). The button asks the shell for the system's open panel and fills the box; it is only there in Bate's own window, so the typed path is the way on another device | | yes |
| Toast | | | #150 |
| Loading | A `.note` saying what is loading; no spinner | | yes |

## Layout

Three regions in one grid: sidebar, main area, side pane.

| Region | Size | Rule |
|---|---|---|
| Sidebar | `--side-w`: 280 px, 220 px under 900 px | Sections with a small heading; rows 44 px high; what is new goes at the bottom |
| Main area | The rest | Every screen is one column of at most `--thread-w` (720 px), centred. The conversation's scroller is as wide as the area, so its bar is at the window's edge |
| Side pane | 480 px, dragged wider or narrower | Under 860 px it lies over the conversation (`pane.ts`); as a sheet with a scrim: #158 |

Widths the page must work at: 1200 px and more, 900 px, and 390 px (a phone, since the page is also served to other devices). Under 640 px the sidebar becomes a strip of projects and bots across the top that scrolls sideways, and the header's tabs scroll sideways.

## Accessibility

- **Contrast**, measured for the token pairs in use (WCAG needs 4.5:1 for text, 3:1 for the edge of a control):

  | Pair | Light | Dark |
  |---|---|---|
  | Text on the page | 17.4 | 16.3 |
  | Text on a bot's message | 15.3 | 13.6 |
  | Secondary text on the page | 6.3 | 7.4 |
  | Secondary text on the chosen row | 4.8 | 5.7 |
  | Text on ink | 17.4 | 16.3 |
  | Link on the page | 5.5 | 7.4 |
  | Link on a bot's message | 4.8 | 6.2 |
  | Error text | 5.6 | 5.9 |
  | Warning text on its fill | 5.7 | 8.8 |
  | Edge of a field | 3.2 | 3.3 |

- **Focus.** One ring, 2 px of `--accent` with a 2 px gap, on every control, shown for the keyboard only (`:focus-visible`). The composer shows it around the whole pill.
- **Keyboard.** Every action is a `<button>` or a link, so Tab reaches it and Enter or Space runs it. A message's actions are hidden until the message is pointed at, and shown when the keyboard is inside it or when the device has no pointer. Shortcuts go by the physical key (`shortcut` in `nav.ts`), never fire while an input method composes text, and do nothing behind the palette: ⌘K palette, ⌘F find in the chat, ⌘N new bot, ⇧⌘N new window, ⌘, the app's settings (also a menu item), ⌘[ and ⌘] (or ⌃⇧Tab and ⌃Tab) the previous and next bot in the sidebar's order, ⌘1 to ⌘9 the first nine bots in that order. Esc goes to the nearest thing that is open: a popover, the find bar or the palette, then the side pane, and only then does it deny a question that offers Deny, or stop the bot. ⌘Enter allows a question once.
- **Colour is never the only sign.** A working bot says "Working…" under its name; an error is words; a round button has an `aria-label` and a tooltip.
- **Motion.** Nothing longer than 100 ms, and none when the system asks for reduced motion.
- **Text size.** Sizes are in px today, so the page does not follow a larger text setting. Not done; it comes with the settings (#155).

Not checked: a screen reader, and the page in the system web view of macOS 13.

## Build or adopt

Built: hand-written CSS on tokens, with React components only where there is behaviour.

- The window is 20 screen files of forms, lists and one conversation. Plain elements with one stylesheet cover them in about 190 lines.
- A component library (Radix, Base UI, shadcn/ui; all MIT) would add a dependency and its own way of styling for what is today two needs: a dialog that keeps the focus, and a select with search. `<dialog>` gives the first. The second is the model popover (#146), written by hand: a search box over a list of `<button role="option">`.
- Adopt one later if a third such need comes up. It must be MIT or Apache-2.0.

## Order of the screens

The shell and the conversation are done with this plan: `main.tsx` and `Chat.tsx` are the example. Every other screen already has the new buttons, fields and type, because those are element styles; what is left is its layout, its states and its own components.

| Order | Screen | Issue |
|---|---|---|
| 1 | Sidebar: projects and bots, what needs you | #142, #144 |
| 2 | Composer and question card | #146, #145 |
| 3 | First run: the sign-in screen, "Set up later", the first bot, the new-bot form with starter cards | #143, done |
| 4 | Room: the composer, Markdown replies, who works, who answers with no `@` | #148, done |
| 5 | Projects | #147, done |
| 6 | Add-ons: skills, connectors, workplaces | #150 |
| 7 | Settings, the bot's and the app's | #155 |
| 8 | Routines | #156 |
| 9 | Memory | #157 |
| 10 | Side pane and surfaces | #158 |
| 11 | Palette, pairing page, messages between bots | #159 |
