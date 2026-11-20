import { useEffect, useState } from "react";
import { CodeBlock, Problem, Toggle, useSaves } from "./Form.tsx";
import { request } from "./service.ts";

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
      <h2 id="settings-devices" tabIndex={-1}>Other devices</h2>
      {!lan && !error("load") && <p className="note">Loading…</p>}
      <Problem text={error("load")} />
      {lan && (
        <Toggle
          label="Let devices on this network open Bate"
          checked={lan.on}
          saved={saved("lan")}
          error={error("lan")}
          onChange={(on) => {
            setCode(undefined);
            act("lan", request("set_lan", { on }));
          }}
        />
      )}
      {lan && !lan.on && <p className="note">Other devices cannot open Bate. Turn the switch on to pair one.</p>}
      {lan?.on && (
        <>
          <p>Open on the other device: {lan.urls?.length ? urls(lan.urls) : "this Mac is on no network"}</p>
          {!!lan.tailscaleUrls?.length && <p>Or from anywhere on your Tailscale network: {urls(lan.tailscaleUrls)}</p>}
          <p>The browser will warn about the certificate. Go on only if it shows this fingerprint (SHA-256):</p>
          {lan.fingerprint && <CodeBlock key={lan.fingerprint} text={lan.fingerprint} label="the fingerprint" />}
          <h3>Pairing</h3>
          {code && left > 0 ? (
            <>
              <p>Enter this code on the other device ({Math.floor(left / 60)}:{String(left % 60).padStart(2, "0")} left):</p>
              <CodeBlock key={code.code} text={code.code} label="the pairing code" />
            </>
          ) : (
            <div className="actions">
              <button onClick={() => run("pair", request<{ code: string; expiresAt: number }>("pair_device").then(setCode))}>Pair a device</button>
            </div>
          )}
          <Problem text={error("pair")} />
        </>
      )}
      {lan?.on && !lan.devices.length && <p className="note">No device is paired yet.</p>}
      {!!lan?.devices.length && (
        <>
          <h3>Paired devices</h3>
          <ul className="rows">
            {lan.devices.map((d) => (
              <li key={d.id}>
                <div className="what">
                  <strong>{d.name}</strong>
                  <small>Last seen {new Date(d.lastSeen).toLocaleString()}</small>
                </div>
                <div className="actions">
                  <button className="small" aria-label={`Remove ${d.name}`} onClick={() => act("remove", request("remove_device", { device: d.id }))}>Remove</button>
                </div>
              </li>
            ))}
          </ul>
          <Problem text={error("remove")} />
        </>
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
