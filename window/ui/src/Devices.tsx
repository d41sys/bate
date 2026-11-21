import { useEffect, useState } from "react";
import { CodeBlock, Group, Problem, Setting, useSaves } from "./Form.tsx";
import { request } from "./service.ts";
import { dayTime } from "./times.ts";

type Lan = {
  on: boolean;
  port?: number;
  urls?: string[];
  tailscaleUrls?: string[];
  fingerprint?: string;
  devices: { id: string; name: string; createdAt: number; lastSeen: number }[];
};

/** Other devices on this network: the switch, where to go, the certificate, pairing, and who is paired. */
export function Devices() {
  const [lan, setLan] = useState<Lan>();
  const [code, setCode] = useState<{ code: string; expiresAt: number }>();
  const [now, setNow] = useState(Date.now());
  const { run, fail, saved, error } = useSaves();
  const load = () => request<Lan>("lan_status").then(setLan, (e: Error) => fail("load", e.message));
  useEffect(() => {
    void load();
    // Devices pair from their side: look again now and then, and count the code down.
    const timer = setInterval(() => {
      setNow(Date.now());
      void load();
    }, 2000);
    return () => clearInterval(timer);
  }, []);
  const act = (key: string, p: Promise<unknown>) => run(key, p).then(load);
  const left = code ? Math.max(0, Math.round((code.expiresAt - now) / 1000)) : 0;
  const urls = (list: string[]) => list.map((u) => <code key={u}>{u} </code>);

  return (
    <>
      <Group id="settings-devices" title="Other devices">
        {!lan && !error("load") && <p className="note">Loading…</p>}
        <Problem text={error("load")} />
        {lan && (
          <Setting id="lan" inline title="Let devices on this network open Bate" saved={saved("lan")} error={error("lan")}>
            <input
              type="checkbox"
              role="switch"
              checked={lan.on}
              onChange={(e) => {
                setCode(undefined);
                act("lan", request("set_lan", { on: e.target.checked }));
              }}
            />
          </Setting>
        )}
        {lan?.on && (
          <>
            <Setting title="Open on the other device" hint={lan.urls?.length ? urls(lan.urls) : "This Mac is on no network."} />
            {!!lan.tailscaleUrls?.length && <Setting title="From anywhere on your Tailscale network" hint={urls(lan.tailscaleUrls)} />}
            {lan.fingerprint && (
              <Setting title="Certificate fingerprint (SHA-256)" hint="The browser warns about the certificate. Go on only if it shows this." wide>
                <CodeBlock key={lan.fingerprint} text={lan.fingerprint} label="the fingerprint" />
              </Setting>
            )}
            {code && left > 0 ? (
              <Setting title={`Enter this code on the other device (${Math.floor(left / 60)}:${String(left % 60).padStart(2, "0")} left)`} wide>
                <CodeBlock key={code.code} text={code.code} label="the pairing code" />
              </Setting>
            ) : (
              <Setting title="Pairing" hint="Shows a code to enter on the other device.">
                <button onClick={() => run("pair", request<{ code: string; expiresAt: number }>("pair_device").then(setCode))}>Pair a device</button>
              </Setting>
            )}
            <Problem text={error("pair")} />
          </>
        )}
      </Group>
      {!!lan?.devices.length && (
        <Group title="Paired devices">
          {lan.devices.map((d) => (
            <Setting key={d.id} title={d.name} hint={`Last seen ${dayTime(d.lastSeen, now)}`}>
              <button className="small" aria-label={`Remove ${d.name}`} onClick={() => act("remove", request("remove_device", { device: d.id }))}>Remove</button>
            </Setting>
          ))}
          <Problem text={error("remove")} />
        </Group>
      )}
    </>
  );
}

/** Asks for the code shown in Bate's window, on a device not yet paired. */
export function Pair({ onPair }: { onPair: (code: string, name: string) => Promise<void> }) {
  const [code, setCode] = useState("");
  const [name, setName] = useState(navigator.platform || "This device");
  const [error, setError] = useState("");
  const [state, setState] = useState<"" | "busy" | "paired">("");
  return (
    <main className="pair">
      <form
        className="card fields stack"
        aria-labelledby="pair-title"
        onSubmit={(e) => {
          e.preventDefault();
          setState("busy");
          setError("");
          onPair(code.trim(), name.trim()).then(() => setState("paired"), (err: Error) => (setError(err.message), setState("")));
        }}
      >
        <h1 id="pair-title">Pair with Bate</h1>
        <p>On your Mac, open Bate's settings and choose “Pair a device”. It shows a 6-digit code: type it here.</p>
        <div className="field">
          <label htmlFor="pair-code">Code</label>
          <input id="pair-code" value={code} onChange={(e) => setCode(e.target.value)} inputMode="numeric" autoComplete="one-time-code" autoFocus required />
          {error && <p className="error" role="alert">{error}</p>}
        </div>
        <div className="field">
          <label htmlFor="pair-name">This device's name</label>
          <input id="pair-name" value={name} onChange={(e) => setName(e.target.value)} maxLength={60} />
          <small>Bate lists it in its settings, so you can remove the device later.</small>
        </div>
        {state === "paired" && <p className="note" role="status">Paired. Opening Bate…</p>}
        <button disabled={state !== ""}>{state === "busy" ? "Pairing…" : "Pair"}</button>
      </form>
    </main>
  );
}
