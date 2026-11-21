import { useState } from "react";
import { adapterFacts, type Adapter, type AdapterPreview } from "./agents.ts";
import { Setting } from "./Form.tsx";
import { Sheet } from "./Sheet.tsx";
import { request } from "./service.ts";

/** The adapter of an agent that Bate can fetch: install it, or look for a newer one on a click, or remove Bate's copy. */
export function AdapterRow({ label, adapter, reload }: { label: string; adapter: Adapter; reload: () => Promise<unknown> }) {
  const [ask, setAsk] = useState<{ preview: AdapterPreview } | { remove: true }>();
  const [error, setError] = useState("");
  const [note, setNote] = useState("");
  const [busy, setBusy] = useState(false);
  // `checking`: only a newer version is worth a sheet; else the row says it is up to date.
  const look = (checking: boolean) => {
    setError("");
    setNote("");
    setBusy(true);
    request<AdapterPreview>("preview_agent", { name: adapter.name }).then(
      (preview) => (setBusy(false), checking && !preview.update ? setNote(`Up to date: version ${preview.version} is the newest listed.`) : setAsk({ preview })),
      (e: Error) => (setBusy(false), setError(e.message)),
    );
  };
  const done = () => void reload();
  return (
    <>
      {adapter.version === null ? (
        <Setting title={`${label} adapter`} hint="Bate can fetch the program that connects it, into its own folder." error={error}>
          <button type="button" disabled={busy} onClick={() => look(false)}>Install…</button>
        </Setting>
      ) : (
        <Setting
          title={`${label} adapter, installed by Bate`}
          hint={<>{`Version ${adapter.version}.${adapter.used ? "" : " Your own copy comes first on the PATH, so this one is not used."}`} <span role="status">{note}</span></>}
          error={error}
        >
          <button type="button" disabled={busy} onClick={() => look(true)}>Check for updates</button>
          <button type="button" disabled={busy} onClick={() => setAsk({ remove: true })}>Remove…</button>
        </Setting>
      )}
      {ask && "preview" in ask && (
        <Sheet
          title={`${ask.preview.installedVersion ? "Update" : "Install"} the ${label} adapter`}
          about={ask.preview.description}
          maker={`Another party's program, not Bate's. Its page: ${ask.preview.home}`}
          facts={adapterFacts(ask.preview)}
          may={["It runs on this Mac, with your access, whenever a bot uses this agent.", "It goes in Bate's own folder, not in your global packages."]}
          blocked={ask.preview.missing ?? undefined}
          action={ask.preview.installedVersion ? "Update" : "Install"}
          go={() => request("install_agent", { name: adapter.name, version: ask.preview.version }).then(done)}
          close={() => setAsk(undefined)}
        />
      )}
      {ask && "remove" in ask && (
        <Sheet
          title={`Remove the ${label} adapter`}
          about="This deletes Bate's copy, in Bate's own folder. A copy you installed yourself is not touched. A bot set to this agent cannot start until an adapter is installed again."
          action="Remove"
          danger
          go={() => request("remove_agent", { name: adapter.name }).then(done)}
          close={() => setAsk(undefined)}
        />
      )}
    </>
  );
}
