import { useEffect, useState } from "react";
import { request } from "./service.ts";

type Lan = {
  on: boolean;
  port?: number;
  urls?: string[];
  fingerprint?: string;
  devices: { id: string; name: string; createdAt: number; lastSeen: number }[];
};

/** Other devices on this network: the switch, where to go, the certificate, pairing, and who is paired. */
export function Devices() {
  const [lan, setLan] = useState<Lan>();
  const [code, setCode] = useState<{ code: string; expiresAt: number }>();
  const [now, setNow] = useState(Date.now());
  const [error, setError] = useState("");
  const load = () => request<Lan>("lan_status").then(setLan, (e: Error) => setError(e.message));
  useEffect(() => {
    void load();
    // Devices pair from their side: look again now and then, and count the code down.
    const timer = setInterval(() => {
      setNow(Date.now());
      void load();
    }, 2000);
    return () => clearInterval(timer);
  }, []);
  const run = (p: Promise<unknown>) => p.then(() => (setError(""), load()), (e: Error) => setError(e.message));
  const left = code ? Math.max(0, Math.round((code.expiresAt - now) / 1000)) : 0;

  return (
    <>
      <h2>Other devices</h2>
      <label>
        <input
          type="checkbox"
          checked={lan?.on ?? false}
          disabled={!lan}
          onChange={(e) => {
            setCode(undefined);
            run(request("set_lan", { on: e.target.checked }));
          }}
        />{" "}
        Let devices on this network open Bate
      </label>
      {lan?.on && (
        <>
          <p>Open on the other device: {lan.urls?.length ? lan.urls.map((u) => <code key={u}>{u} </code>) : "this Mac is on no network"}</p>
          <p>
            The browser will warn about the certificate. Go on only if it shows this fingerprint (SHA-256):{" "}
            <code>{lan.fingerprint}</code>
          </p>
          {code && left > 0 ? (
            <p>
              Pairing code <strong className="code">{code.code}</strong> ({Math.floor(left / 60)}:{String(left % 60).padStart(2, "0")} left)
            </p>
          ) : (
            <button onClick={() => request<{ code: string; expiresAt: number }>("pair_device").then(setCode, (e: Error) => setError(e.message))}>
              Pair a device
            </button>
          )}
        </>
      )}
      {!!lan?.devices.length && (
        <ul className="rows">
          {lan.devices.map((d) => (
            <li key={d.id}>
              {d.name} <small>last seen {new Date(d.lastSeen).toLocaleString()}</small>{" "}
              <button onClick={() => run(request("remove_device", { device: d.id }))}>Remove</button>
            </li>
          ))}
        </ul>
      )}
      {error && <p className="error" role="alert">{error}</p>}
    </>
  );
}

/** Asks for the code shown in Bate's window, on a device not yet paired. */
export function Pair({ onPair }: { onPair: (code: string, name: string) => Promise<void> }) {
  const [code, setCode] = useState("");
  const [name, setName] = useState(navigator.platform || "This device");
  const [error, setError] = useState("");
  return (
    <form
      className="fields pair"
      onSubmit={(e) => {
        e.preventDefault();
        onPair(code.trim(), name.trim()).catch((err: Error) => setError(err.message));
      }}
    >
      <h1>Pair with Bate</h1>
      <p>In Bate on your Mac, open a bot's Settings and choose “Pair a device”.</p>
      <label>
        Code <input value={code} onChange={(e) => setCode(e.target.value)} inputMode="numeric" autoComplete="one-time-code" autoFocus required />
      </label>
      <label>
        This device's name <input value={name} onChange={(e) => setName(e.target.value)} maxLength={60} />
      </label>
      <button type="submit">Pair</button>
      {error && <p className="error" role="alert">{error}</p>}
    </form>
  );
}
