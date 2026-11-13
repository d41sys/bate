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
  /** A few characters, such as an emoji; empty: none. */
  avatar: string;
  /** `#rrggbb`; empty: none. */
  color: string;
};

export type Frequency = "minutes" | "hourly" | "daily" | "weekly" | "monthly" | "yearly";

export type Routine = {
  id?: string;
  name: string;
  prompt: string;
  enabled: boolean;
  schedule?: { frequency: Frequency; interval: number; time: string; timezone?: string };
  webhook: boolean;
  hookUrl?: string;
  nextRunMs?: number;
  lastRun?: { atMs: number; outcome: "sent" | "failed" | "missed"; error?: string };
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

export const KINDS = ["read", "write", "run", "connector"] as const;
export type Kind = (typeof KINDS)[number];
export type Level = "always" | "ask" | "never";
export type Permissions = {
  levels: Record<Kind, Level>;
  grants: { kind: Kind; target: string; atMs: number }[];
};
