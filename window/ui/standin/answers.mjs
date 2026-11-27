// The stand-in's answers: one for every command the page sends (PROTOCOL.md gives each command's
// data), made up and fixed. A value is the command's `data`; a function gets the request's fields.
// A command the page learns to send needs its answer here: `standin.test.mjs` fails until it has one.
// Nothing here is real: names, hosts and paths are invented, and no value is a key.
//
// The bots: Analyst (waits for the person, in the project), Daisy (at work, in the project) and
// Scout (loose, run by an installed agent).

/** `now` is the time the page opened, so that "5 min ago" reads the same in every picture. */
export const answers = (now) => {
  const ago = (minutes) => now - minutes * 60_000;
  const botOf = { b1: "Analyst", b2: "Daisy", b3: "Scout" };

  const project = { id: "p1", name: "Firmware review" };
  const bot = (id, fields) => ({
    id, name: botOf[id], createdAt: ago(60 * 24 * 30), running: true, busy: false, waiting: false, paused: false, wholeDisk: false, network: null,
    instructions: "", stepLimit: 100, webSearch: false, check: "off", checkModel: "", checkBot: "", avatar: "", color: "", ...fields,
  });
  const bots = [
    bot("b1", {
      busy: true, waiting: true, avatar: "search", color: "#4a67d6", model: "gpt-6.2-sol", effort: "medium", project,
      instructions: "You read firmware images and say what they talk to. Show the evidence for each claim.", network: ["updates.example.net", "*.docs.example.net"], webSearch: true,
      check: "bot", checkBot: "b2",
    }),
    bot("b2", { busy: true, avatar: "sprout", color: "#2f9e77", model: "gpt-6.2-sol", effort: "low", instructions: "You take a task from Analyst and finish it in your own folder.", project }),
    bot("b3", { agent: ["scout-agent", "--acp"], instructions: "Looks things up and keeps short notes." }),
  ];
  const projects = [
    {
      id: "p1", name: "Firmware review", folder: "/Users/demo/firmware-review", bots: ["b1", "b2"], lead: "b1", hosts: ["updates.example.net"],
      instructions: "Work from the image in `images/`. Keep every finding in `notes/` with the command that shows it.", workplace: "firmware-lab",
    },
  ];

  // The transcripts. `seq` grows by one for each entry.
  let seq = 0;
  // Each line is 4 seconds after the one before; `minutes` sets the clock to that long ago.
  let at = now;
  const row = (entry, minutes) => ({ seq: ++seq, at: (at = minutes === undefined ? at + 4000 : ago(minutes)), entry });
  const analyst = [
    row({ type: "prompt", nonce: "n1", text: "Where does this firmware check for updates?" }, 30),
    row({ type: "turn_start", nonce: "n1" }),
    row({ type: "thinking", delta: "The image is a squashfs behind a short header. Unpack it, then look for URLs in the update binary." }),
    row({ type: "tool_start", call: "c1", tool: "run", args: { command: "binwalk -e firmware.bin" } }),
    row({ type: "tool_end", call: "c1", output: "DECIMAL   HEXADECIMAL   DESCRIPTION\n64        0x40          SquashFS filesystem, little endian", isError: false }),
    row({ type: "tool_start", call: "c2", tool: "run", args: { command: "strings -n 8 _firmware.bin.extracted/squashfs-root/usr/bin/updater | grep https" } }),
    row({ type: "tool_end", call: "c2", output: "https://updates.example.net/v2/manifest.json", isError: false }),
    row({ type: "tool_start", call: "c3", tool: "plan", args: { steps: [{ step: "Unpack the image", status: "done" }, { step: "Find the update address", status: "done" }, { step: "Check how the manifest is signed", status: "in_progress" }] } }),
    row({ type: "tool_end", call: "c3", output: "ok", isError: false }),
    row({ type: "tool_start", call: "c4", tool: "write", args: { path: "notes/update-check.md", content: "# Update check\n\nAsks `updates.example.net` for a manifest.\n" } }),
    row({ type: "tool_end", call: "c4", output: "Wrote notes/update-check.md", isError: false, undo: "u1" }),
    row({ type: "text", delta: "The updater asks `https://updates.example.net/v2/manifest.json`. It does this at start and then every 6 hours; the interval is a constant in `updater` at offset `0x2f40`.\n\n" }),
    row({ type: "text", delta: "- The request carries the device serial number.\n- The reply is JSON with a `url` and a `sha256`.\n- Nothing in the binary checks a signature yet.\n" }),
    row({ type: "checked", result: "passed", note: "Each claim points at a command's output.", by: "Daisy" }),
    row({ type: "turn_end", reason: "done" }),
    row({ type: "prompt", nonce: "n2", text: "Check whether the manifest is signed before it is used." }, 4),
    row({ type: "turn_start", nonce: "n2" }),
    row({ type: "text", delta: "I will run the updater under a trace to see what it does with the reply." }),
    row({ type: "tool_start", call: "c5", tool: "run", args: { command: "strace -f -e trace=openat,write ./updater --once" } }),
  ];
  const daisy = [
    row({ type: "prompt", nonce: "n3", text: "Decompile `updater` and list the functions that touch the manifest." }, 2),
    row({ type: "turn_start", nonce: "n3" }),
    row({ type: "tool_start", call: "d1", tool: "run", args: { command: "objdump -d usr/bin/updater | grep -c manifest" } }),
    row({ type: "tool_end", call: "d1", output: "14", isError: false }),
    row({ type: "text", delta: "Fourteen references. Reading them one at a time." }),
    row({ type: "tool_start", call: "d2", tool: "read", args: { path: "notes/update-check.md" } }),
  ];
  const scout = [
    row({ type: "prompt", nonce: "n4", text: "What does the vendor say about signed updates?" }, 45),
    row({ type: "turn_start", nonce: "n4" }),
    row({ type: "tool_start", call: "s1", tool: "fetch", args: { url: "https://docs.example.net/updates/signing" } }),
    row({ type: "tool_end", call: "s1", output: "Updates are signed with an offline key. The device checks the signature before it unpacks.", isError: false }),
    row({ type: "text", delta: "The vendor says every update is signed with an offline key and checked before it is unpacked. The page is `docs.example.net/updates/signing`." }),
    row({ type: "turn_end", reason: "done" }),
  ];
  // A second conversation of Analyst's that holds one of everything a chat can show.
  const pasted = Array.from({ length: 12 }, (_, i) => `[   ${31 + i}.9021${i}4] eth0: link is not ready (try ${i + 1})`).join("\n");
  const parts = [
    row({ type: "prompt", nonce: "m0", text: "Summarise the whole log." }, 60 * 26 + 5),
    row({ type: "turn_start", nonce: "m0" }),
    row({ type: "text", delta: "It is 4,200 lines. A summary of all of it would hide what matters." }),
    row({ type: "turn_end", reason: "done" }),
    row({ type: "fork", nonce: "m0" }),
    row({ type: "prompt", nonce: "m1", text: "Read `logs/boot.log` and say why the device takes 40 seconds to start." }),
    row({ type: "turn_start", nonce: "m1" }),
    row({ type: "thinking", delta: "Look at the gaps between the time stamps first." }),
    row({ type: "tool_start", call: "m-1", tool: "read", args: { path: "logs/boot.log" } }),
    row({ type: "tool_end", call: "m-1", output: "[    0.000000] Booting Linux\n[    2.114502] mmc0: new high speed SDHC card\n[   31.902114] eth0: link is not ready", isError: false }),
    row({ type: "tool_start", call: "m-2", tool: "run", args: { command: "dmesg --since boot" } }),
    row({ type: "tool_end", call: "m-2", output: "dmesg: unrecognized option '--since'", isError: true }),
    row({ type: "tool_start", call: "m-3", tool: "edit", args: { path: "notes/boot.md", old: "", new: "## Boot\n" } }),
    row({ type: "tool_end", call: "m-3", output: "Edited notes/boot.md", isError: false, undo: "u2" }),
    row({ type: "tool_start", call: "m-4", tool: "plan", args: { steps: [{ step: "Find the longest gap", status: "done" }, { step: "Find what waits there", status: "in_progress" }, { step: "Say how to shorten it", status: "pending" }] } }),
    row({ type: "tool_end", call: "m-4", output: "ok", isError: false }),
    row({
      type: "text",
      delta: [
        "## Why it is slow",
        "The device waits **29 seconds** for the network before it goes on. The notes are in [notes/boot.md](notes/boot.md); the vendor describes the wait on [its page](https://docs.example.net/boot).",
        "| Phase | Starts at | Takes |\n|---|---|---|\n| Kernel | 0.0 s | 2.1 s |\n| Storage | 2.1 s | 0.8 s |\n| Network wait | 2.9 s | 29.0 s |",
        "> eth0: link is not ready",
        "The wait is set in the start script:",
        "```sh\n# /etc/init.d/S40network\nwait_for_link eth0 --timeout 30\n```",
        "### What to change",
        "1. Lower the timeout to 5 seconds.\n2. Start the network in the background:\n   - the display does not need it;\n   - the updater waits for it by itself.",
        "```c\nstatic int wait_for_link(const char *ifname, int timeout_s) {\n    for (int i = 0; i < timeout_s * 10; i++) {\n        if (link_up(ifname)) return 0;\n        usleep(100000);\n    }\n    return -ETIMEDOUT;\n}\n```",
        "```\nno language is named here\n```",
      ].join("\n\n"),
    }),
    row({ type: "turn_end", reason: "done" }),
    row({ type: "prompt", nonce: "m2", text: `What are these lines?\n\n${pasted}` }, 50),
    row({ type: "turn_start", nonce: "m2" }),
    row({ type: "steered", text: "Only the ones after 35 seconds." }),
    row({ type: "text", delta: "They are the network driver asking again, once a second, until the cable answers." }),
    row({ type: "turn_end", reason: "limit" }),
    row({ type: "prompt", nonce: "m3", text: "Go on." }),
    row({ type: "turn_start", nonce: "m3" }),
    row({ type: "turn_end", reason: "aborted" }),
    row({ type: "prompt", nonce: "m4", text: "Now check the second image." }),
    row({ type: "turn_start", nonce: "m4" }),
    row({ type: "retry", attempt: 1, of: 3, waitMs: 2000, error: "the server answered 503" }),
    row({ type: "turn_end", reason: "error", error: "The model did not answer." }),
  ];
  // By conversation; one that is not here has nothing said in it yet.
  const history = { c1: analyst, d1: daisy, s1: scout, c2: parts };
  const question = {
    question: "q1", title: "Run a command", kind: "run", target: "strace -f -e trace=openat,write ./updater --once", options: ["Approve", "Always", "Deny"], folder: "/Users/demo/firmware-review",
  };
  const questions = { b1: [question], b2: [], b3: [] };
  const current = { b1: "c1", b2: "d1", b3: "s1" };

  const conversation = (id, bot, fields) => ({ id, bot, createdAt: ago(60 * 24), touchedAt: ago(30), title: "", name: "", pinned: false, archived: false, busy: false, ...fields });
  const conversations = {
    b1: [
      conversation("c1", "b1", { title: "Where does the update check go?", pinned: true, busy: true, touchedAt: ago(4) }),
      conversation("c2", "b1", { title: "Reading the boot log", touchedAt: ago(60 * 26) }),
      conversation("c3", "b1", { title: "Old notes on the bootloader", archived: true, touchedAt: ago(60 * 24 * 9) }),
    ],
    b2: [conversation("d1", "b2", { title: "Functions that touch the manifest", busy: true, touchedAt: ago(1) })],
    b3: [conversation("s1", "b3", { title: "Vendor's word on signing", touchedAt: ago(45) }), conversation("s0", "b3", { title: "Release notes", touchedAt: ago(60 * 5) })],
  };

  const attention = [
    { bot: "b1", conversation: "c1", title: "Where does the update check go?", atMs: ago(4), busy: true, questions: [question], line: "", ended: null, error: null },
    { bot: "b2", conversation: "d1", title: "Functions that touch the manifest", atMs: ago(1), busy: true, questions: [], line: "Fourteen references. Reading them one at a time.", ended: null, error: null },
    { bot: "b3", conversation: "s1", title: "Vendor's word on signing", atMs: ago(45), busy: false, questions: [], line: "The vendor says every update is signed with an offline key.", ended: "done", error: null },
    { bot: "b3", conversation: "s0", title: "Release notes", atMs: ago(60 * 5), busy: false, questions: [], line: "", ended: "error", error: "The model did not answer." },
  ];

  const connector = (name, fields) => ({ name, enabled: true, transport: { type: "http", url: `https://${name}.example.net/mcp` }, ...fields });
  const docs = connector("docs", {
    oauth: true, signedIn: true, tools: 4,
    toolNames: ["docs__search_pages", "docs__read_page", "docs__create_page", "docs__delete_page"],
    blocked: ["docs__delete_page"], readOnly: ["docs__search_pages", "docs__read_page"], toConfirm: ["docs__read_page"],
  });
  const tracker = connector("tracker", { transport: { type: "stdio", command: "tracker-mcp", args: ["--stdio"] }, tools: 2, toolNames: ["tracker__list_issues", "tracker__comment"] });
  const connectors = { b1: [docs, tracker], b2: [docs], b3: [] };

  const skill = (name, description, fields) => ({ name, description, author: "Example Lab", enabled: true, ...fields });
  const skills = {
    b1: [skill("firmware-triage", "Unpack an image, list what it talks to and what it runs."), skill("report-writing", "Turn findings into a short report with evidence.", { enabled: false })],
    b2: [skill("firmware-triage", "Unpack an image, list what it talks to and what it runs.")],
    b3: [skill("source-notes", "Keep one note per source, with the address and the date read.", { author: undefined })],
  };

  const routines = {
    b1: [
      {
        id: "r1", name: "Morning digest", prompt: "Summarise what the project's bots did since yesterday.", enabled: true, webhook: false, chat: true,
        schedule: { frequency: "daily", interval: 1, time: "08:00" }, nextRunMs: now + 60 * 60_000 * 14, lastRun: { atMs: ago(60 * 10), outcome: "sent", by: "schedule", tokens: { input: 18200, cached: 12000, output: 640 } },
      },
      {
        id: "r2", name: "Check Daisy's work", prompt: "Read Daisy's last answer and say what is missing.", enabled: true, webhook: false, chat: false,
        after: { bot: "b2", outcome: "done" }, lastRun: { atMs: ago(60 * 3), outcome: "sent", by: "after" },
      },
      {
        id: "r3", name: "New issues", prompt: "A tracker issue was opened or changed: say what it needs.", enabled: false, webhook: false, chat: false,
        watch: { connector: "tracker", tool: "list_issues", arguments: { state: "open" }, every: 15 }, lastCheck: { atMs: ago(20), changed: false, hash: "00", last: "[]" },
      },
    ],
    b2: [],
    b3: [],
  };
  const runs = [
    { atMs: ago(60 * 10), outcome: "sent", by: "schedule", tokens: { input: 18200, cached: 12000, output: 640 } },
    { atMs: ago(60 * 34), outcome: "sent", by: "user" },
    { atMs: ago(60 * 58), outcome: "skipped", reason: "busy" },
  ];

  // Notes, by whose they are.
  const notes = {
    bot: [
      { name: "firmware-layout.md", bytes: 412, modifiedMs: ago(60 * 5) },
      { name: "update-check.md", bytes: 188, modifiedMs: ago(30) },
    ],
    project: [
      { name: "findings.md", bytes: 903, modifiedMs: ago(60 * 2) },
      { name: "glossary.md", bytes: 240, modifiedMs: ago(60 * 24 * 3) },
    ],
  };
  const noteText = {
    "firmware-layout.md": "# Layout\n\n- `0x0000` header\n- `0x0040` SquashFS root\n- `0x7f000` configuration, plain text\n",
    "update-check.md": "# Update check\n\nAsks `updates.example.net` every 6 hours.\n",
    "findings.md": "# Findings\n\n1. The updater checks a manifest, not a signature.\n2. The serial number goes out in the request.\n",
    "glossary.md": "# Words\n\n**Manifest**: the JSON file an update server answers with.\n",
  };

  const log = (lines) => ({ kind: "code", language: "text", lines });
  const surfaceParts = {
    "lab://strings": { kind: "table", columns: ["Offset", "String"], rows: [{ id: "1", cells: ["0x2f40", "https://updates.example.net/v2/manifest.json"] }, { id: "2", cells: ["0x2f90", "X-Device-Serial"] }, { id: "3", cells: ["0x3010", "update interval: %d s"] }] },
    "lab://sections": { kind: "tree", nodes: [{ id: "1", label: "usr", depth: 0 }, { id: "2", label: "bin", depth: 1 }, { id: "3", label: "updater", depth: 2 }, { id: "4", label: "etc", depth: 0 }, { id: "5", label: "update.conf", depth: 1 }] },
    "lab://disassembly": log(["push   rbp", "mov    rbp, rsp", "lea    rdi, [rip+0x1a2f]", "call   fetch_manifest"]),
  };

  const workplace = {
    name: "firmware-lab", version: "1.2.0", changed: false, usedBy: ["p1"], roles: [], archive: "0".repeat(8),
    skills: [{ name: "firmware-triage", description: "Unpack an image, list what it talks to and what it runs.", author: "Example Lab" }],
    manifest: {
      name: "firmware-lab", version: "1.2.0", description: "Unpack firmware images and show their parts.", author: "Example Lab",
      program: { command: "firmware-lab-server", args: ["--stdio"] }, needs: { folders: ["images"], hosts: [], commands: ["binwalk", "objdump"] },
    },
  };

  const adapterPreview = {
    name: "scout-agent", description: "Lets Bate run Scout's agent program.", version: "0.4.1", home: "https://example.net/scout-agent", installedVersion: "0.4.0", update: true,
    packages: 3, bytes: 4_200_000, hosts: ["registry.example.net"], node: 20, runtime: { path: "/usr/local/bin/node", version: "22.1.0" }, missing: null,
  };

  const hello = { protocol: 1 };
  return {
    hello: () => hello,
    // Bots and projects
    list_bots: { bots },
    list_projects: { projects },
    create_bot: ({ name }) => ({ bot: bot("b1", { name }) }),
    set_bot: {},
    delete_bot: {},
    order_bots: {},
    set_paused: {},
    set_project: ({ project }) => ({ project: { id: "p1", ...project } }),
    delete_project: {},
    add_roles: { bots: [] },
    read_roles: {
      roles: [{ path: "", shipped: "lead", name: "Lead", instructions: "You lead the project: split the work and hand each part to the bot that fits it.", skills: [], levels: { run: "ask" }, raises: [] }],
    },
    // Conversations and the chat
    list_conversations: ({ bot: id }) => ({ conversations: conversations[id] ?? [], current: current[id] }),
    new_conversation: { conversation: "c9" },
    set_conversation: {},
    export_chat: { path: "/Users/demo/Library/Application Support/Bate/exports/chat.md" },
    open_bot: ({ bot: id, conversation = current[id], before }) => ({
      bot: bots.find((b) => b.id === id), conversation, history: before ? [] : history[conversation] ?? [], more: false, questions: conversation === current[id] ? questions[id] : [],
      queue: conversation === "c2" ? [{ nonce: "w1", text: "Then compare it with the first image." }] : [],
    }),
    list_queue: { queue: [] },
    list_later: ({ conversation }) => ({ later: conversation === "c2" ? [{ id: 1, conversation: "c2", dueMs: ago(-90), note: "Look at the build server again" }] : [] }),
    cancel_later: {},
    remove_queued: {},
    mark_seen: {},
    prompt: {},
    steer: {},
    abort: {},
    compact: {},
    rewind: {},
    answer: {},
    answer_secret: {},
    attach: { path: "attachments/note.txt" },
    undo_change: {},
    search: { hits: [{ bot: "b1", conversation: "c1", seq: 1, atMs: ago(4), kind: "user", snippet: "Where does this firmware check for updates?" }, { bot: "b3", conversation: "s1", seq: 5, atMs: ago(45), kind: "reply", snippet: "The vendor says every update is signed with an offline key" }] },
    list_attention: { items: attention },
    list_mail: ({ bot: id }) => ({
      mail: id !== "b1" ? [] : [
        {
          seq: 1, from: "b1", to: "b2", state: "answered", atMs: ago(60 * 3), text: "Read `updater` and list the functions that touch the manifest.", answer: "Fourteen, in four groups; the list is in notes/functions.md.",
          check: { command: "test -s notes/functions.md", state: "passed", note: "", output: "" },
        },
        { seq: 2, from: "b2", to: "b1", state: "working", atMs: ago(20), text: "Which key signs the manifest, if any?", check: { command: "grep -q 'ed25519' notes/keys.md", state: null, note: "", output: "" } },
        { seq: 3, from: "b1", to: "b2", state: "waiting", atMs: ago(2), text: "Trace the updater's network calls." },
      ],
    }),
    cancel_mail: {},
    list_changes: {
      source: "git", git: true, more: false,
      files: [{ path: "notes/update-check.md", status: "added" }, { path: "scripts/trace.sh", status: "modified" }, { path: "old/notes.txt", status: "deleted" }],
    },
    read_change: ({ path }) => ({ kind: "text", diff: `--- a/${path}\n+++ b/${path}\n@@ -1,3 +1,4 @@\n # Update check\n \n-Asks the server every day.\n+Asks \`updates.example.net\` every 6 hours.\n+The reply is a manifest, not a signature.\n` }),
    // Models and sign-in
    list_models: { models: ["gpt-6.2-sol", "gpt-6.2-astra", "gpt-6.2-lumen"] },
    set_model: {},
    get_chatgpt: { signedIn: true },
    sign_in_chatgpt: { url: "https://auth.example.net/sign-in" },
    sign_out_chatgpt: {},
    get_provider: { url: null, hasKey: false, fromEnv: false },
    set_provider: {},
    // Routines
    list_routines: ({ bot: id }) => ({ routines: routines[id] ?? [] }),
    set_routine: ({ routine }) => ({ routine }),
    run_routine: { skipped: null },
    delete_routine: {},
    list_routine_runs: { runs },
    // Memory
    list_memory: (f) => ({ notes: "bot" in f ? notes.bot : notes.project }),
    read_memory: ({ name }) => ({ text: noteText[name] ?? "" }),
    write_memory: {},
    delete_memory: {},
    list_deleted_memory: { notes: [{ name: "draft-plan.md", bytes: 96, atMs: ago(60 * 7), by: "user", id: "v3" }] },
    list_memory_versions: { versions: [{ id: "v2", atMs: ago(60 * 2), bytes: 410, by: "bot" }, { id: "v1", atMs: ago(60 * 30), bytes: 380, by: "user" }] },
    restore_memory_version: {},
    // Channels
    get_channels: {
      channels: [
        { kind: "telegram", hasToken: true, pairing: false, chats: [{ chat: "1001", name: "Sam's phone" }], error: null },
        { kind: "discord", hasToken: false, pairing: false, chats: [], error: null },
      ],
    },
    set_channel: {},
    pair_channel: { code: "482913", expiresAt: now + 5 * 60_000 },
    remove_chat: {},
    // Connectors, skills, workplaces
    list_connectors: ({ bot: id }) => ({ connectors: connectors[id] ?? [] }),
    set_connector: ({ connector: c }) => ({ connector: c }),
    delete_connector: {},
    sign_in_connector: { url: "https://docs.example.net/sign-in" },
    sign_out_connector: {},
    block_tool: {},
    confirm_read_tools: {},
    list_skills: ({ bot: id }) => ({ skills: skills[id] ?? [] }),
    set_skill_enabled: {},
    preview_skill: { skill: { name: "log-reading", description: "Read a log and say what failed first." }, entries: 3, bytes: 2400 },
    preview_skill_catalog: { skill: { name: "log-reading", description: "Read a log and say what failed first.", author: "Example Lab" }, entries: 3, bytes: 2400, entry: { sha256: "1".repeat(8), author: "Example Lab" } },
    install_skill: { skill: { name: "log-reading", description: "Read a log and say what failed first." } },
    install_skill_catalog: { skill: { name: "log-reading", description: "Read a log and say what failed first." } },
    delete_skill: {},
    list_workplaces: { workplaces: [workplace] },
    preview_workplace: { manifest: workplace.manifest, downloadBytes: 0, hosts: [] },
    install_workplace: { workplace },
    approve_workplace: {},
    remove_workplace: {},
    preview_catalog: {
      entry: { version: "1.3.0", sha256: "2".repeat(8), author: "Example Lab" }, manifest: { ...workplace.manifest, version: "1.3.0" }, downloadBytes: 18_000_000, hosts: ["downloads.example.net"],
      newly: { folders: [], hosts: ["downloads.example.net"], commands: [], downloads: [{ dest: "tools/disasm", size: 18_000_000 }] },
    },
    install_catalog: { workplace },
    list_catalog: ({ kind }) => ({
      catalog:
        kind === "connectors"
          ? [{ name: "calendar", description: "Read and add calendar events.", author: "Example Lab", home: "https://example.net/calendar", transport: { type: "http", url: "https://calendar.example.net/mcp" }, oauth: true }]
          : kind === "skills"
            ? [{ name: "log-reading", description: "Read a log and say what failed first.", author: "Example Lab", size: 2400, sha256: "1".repeat(8) }]
            : [{ name: "firmware-lab", description: "Unpack firmware images and show their parts.", author: "Example Lab", version: "1.3.0", size: 90_000, sha256: "2".repeat(8), installedVersion: "1.2.0", update: true }],
    }),
    // Projects: room, surfaces
    list_room: {
      room: [
        { seq: 1, from: "", to: "b1", text: "Start with the update binary: what does it talk to?", atMs: ago(60 * 3) },
        { seq: 2, from: "b1", to: "", text: "It asks `updates.example.net` for a manifest. I gave Daisy the job of listing the functions that touch it.", atMs: ago(60 * 3 - 4) },
        { seq: 3, from: "", to: "b2", text: "@Daisy keep the list in notes/functions.md", atMs: ago(60 * 2) },
      ],
    },
    send_room: { seq: 4, to: "b1", queued: false },
    list_surfaces: { surfaces: [{ uri: "lab://strings", name: "Strings", description: "Text found in the image", kind: "table" }, { uri: "lab://sections", name: "Files", kind: "tree" }, { uri: "lab://disassembly", name: "Disassembly", kind: "code" }] },
    read_surface: ({ uri }) => ({ offset: 0, total: 3, select: "select", open: "open", actions: [{ tool: "rename", label: "Rename", input: true }], ...surfaceParts[uri] }),
    act_surface: { ok: true, text: "Done." },
    watch_surfaces: {},
    // Settings of a bot
    get_usage: { today: { input: 182_000, cached: 120_000, output: 6_400, unreported: 0 }, month: { input: 2_900_000, cached: 1_800_000, output: 91_000, unreported: 2 }, limit: { day: 500_000, month: 6_000_000, stops: "unattended" } },
    set_token_limit: {},
    get_permissions: {
      levels: { read: "always", write: "ask", run: "ask", connector: "read_only", desktop: "never", apple_read: "ask", apple_change: "ask" },
      grants: [{ kind: "run", target: "ls -l", atMs: ago(60 * 24) }],
      conversationGrants: [{ kind: "write", target: "notes/update-check.md", atMs: ago(60), conversation: "c1", title: "Where does the update check go?" }],
      blocked: ["docs__delete_page"],
    },
    set_permission: {},
    delete_grant: {},
    list_secrets: { secrets: [{ name: "portal-login", site: "portal.example.net" }] },
    remove_secret: {},
    set_disk_access: {},
    set_network: {},
    hand_back_browser: {},
    set_agent: {},
    list_agents: {
      agents: [{ label: "Scout agent", command: ["scout-agent", "--acp"], path: "/usr/local/bin/scout-agent", adapter: { name: "scout-agent", version: "0.4.0", used: true } }],
      missing: [{ label: "Other agent", install: "npm install -g other-agent" }],
    },
    preview_agent: adapterPreview,
    install_agent: { name: "scout-agent", version: "0.4.1" },
    remove_agent: {},
    export_bot: { path: "/Users/demo/Library/Application Support/Bate/exports/bate-bot-analyst.batebot", check: [{ name: "tracker", command: "tracker-mcp --stdio" }] },
    preview_bot_file: {
      name: "Analyst", instructionsLength: 120, notes: 2, sha256: "3".repeat(8), connectors: [{ name: "docs", address: "https://docs.example.net/mcp" }],
      skills: [{ name: "firmware-triage", description: "Unpack an image.", files: 3, runsScripts: false }], routines: [{ name: "Morning digest", schedule: { frequency: "daily", interval: 1, time: "08:00" }, webhook: false, chat: true }],
    },
    import_bot: { bot: bot("b1", {}) },
    // The app's settings
    export_report: { path: "/Users/demo/Library/Application Support/Bate/exports/report.zip" },
    get_keychain: { refused: false },
    ask_keychain: {},
    get_helper: { installed: true, path: "/Applications/Bate Helper.app", connected: true, accessibility: true, screenRecording: false, fullDiskAccess: null },
    open_helper_settings: {},
    get_quick_ask: { shortcut: "Ctrl+Alt+Space", selection: true, screenshot: false },
    set_quick_ask: { shortcut: "Ctrl+Alt+Space", selection: true, screenshot: false },
    quick_context: { quote: "Nothing in the binary checks a signature yet.", quoteError: null, windows: null, windowsError: null },
    quick_shot: { path: "attachments/shot.png" },
    get_update: { state: "ready", current: "0.9.0", available: "0.9.1", error: null },
    check_update: { state: "ready", current: "0.9.0", available: "0.9.1", error: null },
    install_update: { state: "waiting", current: "0.9.0", available: "0.9.1", error: null },
    lan_status: {
      on: true, port: 8443, urls: ["https://192.168.1.20:8443"], tailscaleUrls: [], fingerprint: "AB:12:CD:34:EF:56:AB:12:CD:34:EF:56:AB:12:CD:34",
      devices: [{ id: "dev1", name: "Sam's phone", createdAt: ago(60 * 24 * 12), lastSeen: ago(12) }],
    },
    set_lan: {},
    pair_device: { code: "739204", expiresAt: now + 5 * 60_000 },
    remove_device: {},
  };
};
