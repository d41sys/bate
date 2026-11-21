import { useEffect, useState } from "react";
import { listen, request } from "./service.ts";

/** One quiet line, only while macOS has refused the service its secrets: the service asks again when told to, not by itself. */
export function KeychainNotice() {
  const [refused, setRefused] = useState(false);
  useEffect(() => {
    // Another device on the network is not let ask: the line stays hidden there.
    const load = () => void request<{ refused: boolean }>("get_keychain").then((k) => setRefused(k.refused), () => {});
    load();
    return listen((frame) => frame.type === "keychain_changed" && load());
  }, []);
  if (!refused) return null;
  return (
    <p className="note" role="status">
      Bate asks macOS again only when you say so. <button type="button" className="small" onClick={() => void request("ask_keychain").catch(() => {})}>Ask again</button>
    </p>
  );
}
