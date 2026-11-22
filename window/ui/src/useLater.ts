import { useEffect, useState } from "react";
import { listen, request } from "./service.ts";
import type { Later } from "./types.ts";

/** The later turns a bot has waiting (those of one conversation, if it is given), soonest first; they load again when they change. */
export function useLater(bot: string, conversation?: string) {
  const [later, setLater] = useState<Later[]>([]);
  const [error, setError] = useState("");
  useEffect(() => {
    let gone = false;
    const load = () =>
      request<{ later: Later[] }>("list_later", { bot, conversation }).then(
        (d) => gone || (setLater(d.later), setError("")),
        (e: Error) => gone || setError(e.message),
      );
    load();
    const stop = listen((frame) => {
      if (frame.type === "later_changed" && frame.bot === bot) load();
    });
    return () => {
      gone = true;
      stop();
    };
  }, [bot, conversation]);
  return { later, error };
}
