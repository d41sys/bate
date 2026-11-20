// Shapes from PROTOCOL.md and `proto`, as the service sends them.

export type Bot = {
  id: string;
  name: string;
  createdAt: number;
  running: boolean;
  busy: boolean;
  wholeDisk: boolean;
  agent?: string[];
  browserTaken?: boolean;
  instructions: string;
  /** Tool rounds a turn may take before it stops and asks to continue. */
  stepLimit: number;
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
};

export const KINDS = ["read", "write", "run", "connector", "desktop", "apple_read", "apple_change"] as const;
export type Kind = (typeof KINDS)[number];
export type Level = "always" | "ask" | "never";
export type Permissions = {
  levels: Record<Kind, Level>;
  grants: { kind: Kind; target: string; atMs: number }[];
};

export type Group = {
  id?: string;
  name: string;
  /** A bot's id. */
  lead: string;
  /** The other bots' ids, 1 to 20; not the lead. */
  members: string[];
};

export type Project = {
  id?: string;
  name: string;
  /** The folder's full path; empty in a new project for one Bate makes. */
  folder: string;
  /** The ids of the bots in it, at most 20. A bot is in at most one project. */
  bots: string[];
  /** What every bot of the project follows after its own instructions, at most 8000 characters. */
  instructions?: string;
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
  } | null;
  skills: Skill[];
  /** Whole paths of its role files, for "Add bots from role files". */
  roles: string[];
  /** The files are not what was approved. */
  changed: boolean;
  error?: string;
  /** Ids of the projects that have it on. */
  usedBy: string[];
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

/** A prompt the service holds until the bot is free. */
export type QueuedPrompt = { nonce: string; text: string };

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
  actions: { tool: string; label: string }[];
} & (
  | { kind: "table"; columns: string[]; rows: { id: string; cells: string[] }[] }
  | { kind: "tree"; nodes: { id: string; label: string; depth: number }[] }
  | { kind: "code"; language: string; lines: string[] }
);
