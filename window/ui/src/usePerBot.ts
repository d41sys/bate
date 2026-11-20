import { useEffect, useState } from "react";
import { listen, request } from "./service.ts";
import type { Bot } from "./types.ts";

/** What each bot has of one kind (`list_skills` or `list_connectors`), read again when any bot's list changes. */
export function usePerBot<T>(bots: Bot[], command: string, key: string, changed: string) {
  const [lists, setLists] = useState<{ bot: string; items: T[] }[]>([]);
  const [error, setError] = useState("");
  const ids = bots.map((b) => b.id).join();
  useEffect(() => {
    const load = () =>
      Promise.all(bots.map((b) => request<Record<string, T[]>>(command, { bot: b.id }).then((d) => ({ bot: b.id, items: d[key] })))).then(
        (all) => (setLists(all), setError("")),
        (e: Error) => setError(e.message),
      );
    void load();
    return listen((frame) => {
      if (frame.type === changed) void load();
    });
  }, [ids, command, key, changed]);
  return { lists, error };
}
