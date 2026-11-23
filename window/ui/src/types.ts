// Shapes from PROTOCOL.md and `proto`, as the service sends them.

export type Bot = {
  id: string;
  name: string;
  createdAt: number;
  running: boolean;
  busy: boolean;
  /** One of its questions is open: it waits for the person's answer. */
  waiting: boolean;
  /** The user paused it: its turn is stopped, its routines skip, other bots' messages wait, and it takes none from the user. */
  paused: boolean;
  wholeDisk: boolean;
  /** The hosts the bot's commands may reach (its project's are added); `null`: any. */
  network: string[] | null;
  agent?: string[];
  browserTaken?: boolean;
  instructions: string;
  /** Tool rounds a turn may take before it stops and asks to continue. */
  stepLimit: number;
  /** Whether the model provider's own web search is offered (it exists only on the ChatGPT plan). */
  webSearch: boolean;
  /** What checks the bot's answers to you before the turn ends: `off`; `own`, its own model; `model`, the one in `checkModel`; `bot`, the bot `checkBot` of its project (`check`). */
  check: "off" | "own" | "model" | "bot";
  checkModel: string;
  checkBot: string;
  /** Why the bot in `checkBot` cannot check now (gone, left the project, signed out); then the bot's answers are not checked. */
  checkBroken?: string;
  /** A few characters, such as an emoji; empty: none. */
  avatar: string;
  /** `#rrggbb`; empty: none. */
  color: string;
  /** Chosen with `set_model`; absent: the engine's default. */
  model?: string;
  effort?: string;
  /** The project the bot is in, if any. */
  project?: { id: string; name: string };
};

export type Frequency = "minutes" | "hourly" | "daily" | "weekly" | "monthly" | "yearly";

/** What a model server said its requests used. `input` is all the input; `cached` is the part of it that came from the server's cache. */
export type Tokens = { input: number; cached: number; output: number };

/** `reason`: why a skipped run was not run (`paused`, `signed_out`, `backoff`, `limit`, `busy`, `rate`, `precheck`); `detail`: the connector, the token limit's words, or the first line the precheck printed; `by`: `user` for a run the person started, `after` for one that began when another bot's turn ended; `tokens`: what the run's requests used, when the server said. */
export type RoutineRun = { atMs: number; outcome: "sent" | "failed" | "missed" | "limit" | "skipped"; error?: string; reason?: string; detail?: string; by?: string; tokens?: Tokens };

/** Tokens used over a period, and how many of its requests the server said nothing of (`get_usage`). */
export type Tally = Tokens & { unreported: number };

/** What `get_usage` says: a limit that is `null` is not set. `stops`: which work the limit holds. */
export type Usage = { today: Tally; month: Tally; limit: { day: number | null; month: number | null; stops: "unattended" | "everything" } };

/** Output only: a scheduled routine whose last runs failed is held to fewer runs. */
export type Backoff = { failures: number; nextRunMs?: number };

/** A routine that starts when a turn of the bot `bot` (its id) ends this way: `failed` is an error or a stop at a limit. */
export type After = { bot: string; outcome: "done" | "failed" | "any" };

/** A routine that starts when a tool of one of the bot's connectors answers differently: the service calls `tool` (the connector's own name for it) with `arguments` every `every` minutes (5 to 1440), with no model. */
export type Watch = { connector: string; tool: string; arguments: Record<string, unknown>; every: number };

/** Output only: the last check of a watch. `changed`: the answer was not the one before; `error`: why it made no call, or the call failed; `last`: the answer kept, cut to 4096 bytes. */
export type Check = { atMs: number; changed: boolean; error?: string; hash: string; last: string };

export type Routine = {
  id?: string;
  name: string;
  prompt: string;
  enabled: boolean;
  schedule?: { frequency: Frequency; interval: number; time: string; timezone?: string };
  after?: After;
  webhook: boolean;
  chat: boolean;
  precheck?: string;
  watch?: Watch;
  hookUrl?: string;
  nextRunMs?: number;
  lastRun?: RoutineRun;
  backoff?: Backoff;
  lastCheck?: Check;
};

export type Connector = {
  name: string;
  enabled: boolean;
  transport:
    | { type: "http"; url: string; headers?: Record<string, string> }
    | { type: "stdio"; command: string; args?: string[]; env?: Record<string, string> };
  oauth?: boolean;
  clientId?: string;
  signedIn?: boolean;
  /** What the bot's engine said when it could not load the connector (output only). */
  error?: string;
  /** How many tools the bot's engine got from the connector when it loaded it (output only); absent: not loaded in this run. */
  tools?: number;
  /** Those tools by the model's name, `<connector>__<tool>`, blocked ones too (output only). */
  toolNames?: string[];
  /** The tools of this connector that are blocked for the bot (output only). */
  blocked?: string[];
  /** The tools its server marks read-only, a claim of the server's (output only). */
  readOnly?: string[];
  /** Marked read-only and not yet confirmed by the person, while the bot's level for connectors is "read_only": they still ask (output only). */
  toConfirm?: string[];
};

export const KINDS = ["read", "write", "run", "connector", "desktop", "apple_read", "apple_change"] as const;
export type Kind = (typeof KINDS)[number];
export type Level = "always" | "read_only" | "ask" | "never";
export type Permissions = {
  levels: Record<Kind, Level>;
  grants: { kind: Kind; target: string; atMs: number }[];
  /** Answers "Allow in this conversation", each with the conversation it is for and that conversation's name. */
  conversationGrants: { kind: Kind; target: string; atMs: number; conversation: string; title: string }[];
  /** The connector tools blocked for the bot, `<connector>__<tool>`. */
  blocked: string[];
};

export type Project = {
  id?: string;
  name: string;
  /** The folder's full path; empty in a new project for one Bate makes. */
  folder: string;
  /** The ids of the bots in it, at most 20. A bot is in at most one project. */
  bots: string[];
  /** The bot a room message with no `@name` goes to: one of `bots`; the first when not set. */
  lead?: string;
  /** What every bot of the project follows after its own instructions, at most 8000 characters. */
  instructions?: string;
  /** Hosts added for each of its bots that has a network limit: `host`, `*.domain`, with an optional `:port`. */
  hosts?: string[];
  /** A program the service runs once for the whole project; every bot gets its tools. */
  connector?: { name: string; command: string; args: string[] };
  /** The name of an installed workplace whose program is the project's connector; not together with `connector`. */
  workplace?: string;
  /** Output only: why that program is not running now. */
  connectorError?: string;
};

/** An installed workplace (`list_workplaces`). */
export type Workplace = {
  name: string;
  version: string;
  /** `null`: the manifest is malformed; `error` says how. */
  manifest: {
    name: string;
    version: string;
    description?: string;
    /** A claim in the manifest, never checked. */
    author?: string;
    program: { command: string; args: string[] };
    needs: { folders: string[]; hosts: string[]; commands: string[] };
    downloads?: { url: string; size: number; sha256: string; dest: string; unpack?: string }[];
  } | null;
  skills: Skill[];
  /** Whole paths of its role files, for "Add bots from role files". */
  roles: string[];
  /** The files are not what was approved. */
  changed: boolean;
  error?: string;
  /** Ids of the projects that have it on. */
  usedBy: string[];
  /** SHA-256 of the catalog's archive it was installed from, if it came from the catalog. */
  archive?: string;
};

/** What a role file would create (`read_roles`). */
export type Role = {
  path: string;
  name: string;
  instructions: string;
  model?: string;
  effort?: string;
  skills: string[];
  levels: Record<string, string>;
  /** Kinds whose level allows more than a new bot's: applied only with `raise`. */
  raises: string[];
  /** Set for a role that ships with the app, which has no file: its name for `read_roles` and `add_roles`. */
  shipped?: string;
};

/** `author` is what the skill's file says: a claim, never checked. */
export type Skill = { name: string; description: string; author?: string };

/** A skill as `list_skills` gives it: whether it is on for that bot (off: installed, but the model is not told of it). */
export type InstalledSkill = Skill & { enabled: boolean };

/** What `preview_skill` says of a skill folder: the skill and how much an install would copy. `preview_skill_catalog` adds the entry of the list it came from. */
export type SkillPreview = { skill: Skill; entries: number; bytes: number; entry?: { sha256: string; author?: string } };

/** An entry of the list of connectors (`list_catalog` with `kind: "connectors"`): what the "New connector" form is filled in with. Nothing here is a value of the person's own: `fill` names what they add. */
export type ConnectorListed = {
  name: string;
  description: string;
  /** A claim in the list, never checked. */
  author?: string;
  /** The maker's page the address or command was read from. */
  home: string;
  transport: Connector["transport"];
  oauth?: boolean;
  fill?: string[];
};

/** An entry of the list of skills (`list_catalog` with `kind: "skills"`). */
export type SkillListed = { name: string; description: string; author?: string; size: number; sha256: string };

/** What `preview_bot_file` says a bot file holds. `sha256` is given back to `import_bot`. */
export type BotFilePreview = {
  name: string;
  instructionsLength: number;
  skills: { name: string; description: string; author?: string; files: number; runsScripts: boolean }[];
  routines: { name: string; schedule: Routine["schedule"] | null; webhook: boolean; chat: boolean }[];
  notes: number;
  connectors: { name: string; address: string }[];
  sha256: string;
};

/** A prompt the service holds until the bot is free. */
export type QueuedPrompt = { nonce: string; text: string };

/** A turn the bot set for itself with `check_back`, waiting for its time. */
export type Later = { id: number; conversation: string; dueMs: number; note: string };

/** One of a bot's conversations (`list_conversations`). */
export type Conversation = {
  id: string;
  bot: string;
  createdAt: number;
  /** When a prompt last went into it, in Unix milliseconds. */
  touchedAt: number;
  /** Made after the first reply; empty before. */
  title: string;
  /** The person's own; empty: the title shows. */
  name: string;
  pinned: boolean;
  archived: boolean;
  /** The bot works in it now. */
  busy: boolean;
};

/** A surface a project's program offers (`list_surfaces`). */
export type Surface = { uri: string; name: string; description?: string; kind: "table" | "tree" | "code" };

/** One part of a surface's content (`read_surface`): data, to be shown as text. */
export type SurfacePart = {
  /** Where this part begins in the whole, and how many items the whole has. */
  offset: number;
  total: number;
  /** The tools the person's choosing and opening call, and the named actions on offer. */
  select?: string;
  open?: string;
  /** `input`: the action takes words the person writes. */
  actions: { tool: string; label: string; input?: boolean }[];
} & (
  | { kind: "table"; columns: string[]; rows: { id: string; cells: string[] }[] }
  | { kind: "tree"; nodes: { id: string; label: string; depth: number }[] }
  | { kind: "code"; language: string; lines: string[] }
);

/** What `preview_workplace` says of a folder: its manifest and what installing it would fetch. */
export type WorkplacePreview = {
  manifest: NonNullable<Workplace["manifest"]>;
  downloadBytes: number;
  hosts: string[];
};

/** An entry of the catalog and what is installed of it (`list_catalog`). */
export type CatalogEntry = {
  name: string;
  description: string;
  /** A claim in the list, never checked. */
  author?: string;
  version: string;
  size: number;
  /** The archive the entry pins: sent back to install exactly what was shown. */
  sha256: string;
  installedVersion: string | null;
  /** What is installed is not the archive the entry pins. */
  update: boolean;
};

/** What `preview_catalog` says of an entry's archive: what it needs that the installed one did not. */
export type CatalogPreview = WorkplacePreview & {
  entry: { version: string; sha256: string; author?: string };
  newly: { folders: string[]; hosts: string[]; commands: string[]; downloads: { dest: string; size: number }[] };
};
