// Shapes from PROTOCOL.md and `proto`, as the service sends them.

export type Bot = {
  id: string;
  name: string;
  createdAt: number;
  running: boolean;
  busy: boolean;
  /** One of its questions is open: it waits for the person's answer. */
  waiting: boolean;
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

export type RoutineRun = { atMs: number; outcome: "sent" | "failed" | "missed" | "limit"; error?: string };

export type Routine = {
  id?: string;
  name: string;
  prompt: string;
  enabled: boolean;
  schedule?: { frequency: Frequency; interval: number; time: string; timezone?: string };
  webhook: boolean;
  chat: boolean;
  hookUrl?: string;
  nextRunMs?: number;
  lastRun?: RoutineRun;
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
};

export const KINDS = ["read", "write", "run", "connector", "desktop", "apple_read", "apple_change"] as const;
export type Kind = (typeof KINDS)[number];
export type Level = "always" | "ask" | "never";
export type Permissions = {
  levels: Record<Kind, Level>;
  grants: { kind: Kind; target: string; atMs: number }[];
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
};

export type Skill = { name: string; description: string };

/** What `preview_skill` says of a skill folder: the skill and how much an install would copy. */
export type SkillPreview = { skill: Skill; entries: number; bytes: number };

/** A prompt the service holds until the bot is free. */
export type QueuedPrompt = { nonce: string; text: string };

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
  entry: { version: string; sha256: string };
  newly: { folders: string[]; hosts: string[]; commands: string[]; downloads: { dest: string; size: number }[] };
};
