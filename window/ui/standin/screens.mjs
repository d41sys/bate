// The window's screens, one entry each, in the words of the UI and `GLOSSARY.md`.
//   name   : the address (`/<name>` on the stand-in's server) and the file name of a picture
//   title  : the screen's name as the UI says it
//   steps  : how to reach it from the start, one click each, by stable selector: a selector, or
//            [selector, text it holds]; `{ code: "KeyK", metaKey: true }` is a key chord
//   parts  : selectors of the screen's main parts, an id where the page has one; the page must show all
//   answers: made-up answers that differ for this screen (see `answers.mjs`); globals: what the shell sets
// `standin.test.mjs` checks that every selector here is in the page's code.

const side = (text) => [".sidebar .row", text];
const bot = (name, tab) => [side(name), `#tab-${tab}`];
const project = (tab) => [[".sidebar .row.plain", "Firmware review"], `#tab-${tab}`];
const addons = (tab) => [[".sidebar .foot .row", "Add-ons"], `#tab-${tab}`];
const SETTINGS = [".sidebar .foot .row", "Settings"];
// The conversation chip's list, open.
const chip = [".conversation .chip", "[aria-label='Conversations of this bot']"];

export const screens = [
  { name: "activity", title: "Activity", steps: [[".sidebar .row.plain", "Activity"]], parts: ["#attention-needs", "#attention-working", "#attention-done", "#attention-failed", ".attention .question"] },
  {
    name: "first-run", title: "First run (Welcome to Bate)", steps: [], parts: [".first-run"],
    answers: { get_chatgpt: { signedIn: false }, list_bots: { bots: [] }, list_projects: { projects: [] }, list_attention: { items: [] } },
  },
  { name: "new-bot", title: "New bot", steps: ["button[aria-label='New bot']"], parts: [".starters", ".fields"] },
  { name: "new-project", title: "New project", steps: ["button[aria-label='New project']"], parts: ["#dialog-title", "#project-folder", "#new-bot-name", "#lead-role"] },

  { name: "chat", title: "Conversation (a bot waits for you)", steps: bot("Analyst", "chat"), parts: ["#tab-chat", ".conversation", ".question", "form textarea", "details.run"] },
  { name: "chat-calls", title: "Conversation (what the bot did, opened)", steps: [...bot("Scout", "chat"), "details.run > summary", "details.run[open] li summary"], parts: ["details.run[open] li details[open] pre"] },
  { name: "chat-busy", title: "Conversation (a bot at work)", steps: bot("Daisy", "chat"), parts: ["#tab-chat", "form textarea", "[aria-label='Stop']"] },
  { name: "chat-agent", title: "Conversation (a bot on an installed agent)", steps: bot("Scout", "chat"), parts: ["#tab-chat", "form textarea"] },
  { name: "mail", title: "Messages with other bots", steps: [...bot("Analyst", "chat"), "details.mail summary"], parts: ["details.mail .check"] },
  { name: "conversations", title: "Conversations of a bot", steps: [...bot("Analyst", "chat"), chip[0]], parts: [chip[1]] },
  { name: "changes", title: "Changes", steps: [...bot("Analyst", "chat"), chip[0], [`${chip[1]} .foot .row`, "Show changes"]], parts: [".pane"] },
  { name: "routines", title: "Routines", steps: bot("Analyst", "routines"), parts: ["#tabpanel .rows"] },
  { name: "memory", title: "Memory (a bot's notes)", steps: bot("Analyst", "memory"), parts: ["#tabpanel"] },
  { name: "channels", title: "Channels", steps: bot("Analyst", "channels"), parts: ["#tabpanel"] },
  { name: "bot-settings", title: "Settings (a bot's)", steps: bot("Analyst", "settings"), parts: ["#bot-name", "#bot-instructions", "#bot-limit-day", "#perm-run", "#bot-network"] },
  { name: "bot-settings-agent", title: "Settings (a bot on an installed agent)", steps: bot("Scout", "settings"), parts: ["#bot-name", "#bot-engine"] },

  { name: "project-room", title: "Room", steps: project("room"), parts: ["#tabpanel", ".composer", ".bubble"] },
  { name: "project-bots", title: "Bots (of a project)", steps: project("bots"), parts: ["#tabpanel"] },
  { name: "project-surfaces", title: "Surfaces", steps: project("surfaces"), parts: [".surfaces"] },
  { name: "surface", title: "Surface (opened beside)", steps: [...project("surfaces"), [".surfaces button", "Strings"]], parts: [".pane"] },
  { name: "project-memory", title: "Memory (a project's notes)", steps: project("memory"), parts: ["#tabpanel"] },
  { name: "project-settings", title: "Settings (a project's)", steps: project("settings"), parts: ["#project-name", "#project-instructions", "#project-lead"] },

  { name: "connectors", title: "Add-ons: Connectors", steps: [...addons("connectors"), "#tabpanel details > summary", ["#tabpanel summary", "marked read-only"]], parts: ["#tabpanel .rows", "#tabpanel details details[open]"] },
  { name: "skills", title: "Add-ons: Skills", steps: addons("skills"), parts: ["#tabpanel"] },
  { name: "workplaces", title: "Add-ons: Workplaces", steps: addons("workplaces"), parts: ["#tabpanel"] },
  { name: "workplaces-catalog", title: "Add-ons: Workplaces, the catalog", steps: [...addons("workplaces"), ["#tabpanel button", "Show the catalog"]], parts: ["#tabpanel .rows"] },

  {
    name: "settings", title: "Settings (the app's)", steps: [SETTINGS],
    parts: ["#settings-chatgpt", "#settings-model", "#settings-devices", "#settings-helper", "#settings-quick", "#settings-updates", "#settings-report"],
  },
  { name: "palette", title: "Palette", steps: [{ code: "KeyK", metaKey: true }], parts: ["#palette-list"] },
  { name: "quick-ask", title: "Quick Ask", steps: [], parts: ["textarea"], globals: { __BATE_QUICK__: true } },
];
