import { useEffect, useState } from "react";
import { listen, request } from "./service.ts";
import type { Bot } from "./types.ts";

type Line = { seq: number; from: string; to: string; text: string; atMs: number };

/** One conversation with the project's bots: `@name` addresses one, no name goes to the lead. */
export function Room({ project, bots }: { project: { id: string; name: string }; bots: Bot[] }) {
  const [room, setRoom] = useState<Line[]>([]);
  const [text, setText] = useState("");
  const [error, setError] = useState("");

  useEffect(() => {
    const load = () => request<{ room: Line[] }>("list_room", { projectId: project.id }).then((d) => setRoom(d.room), (e) => setError(e.message));
    load();
    return listen((frame) => {
      if (frame.type === "room_changed" && frame.projectId === project.id) load();
    });
  }, [project.id]);

  const members = bots.filter((b) => b.project?.id === project.id);
  const name = (id: string) => bots.find((b) => b.id === id)?.name ?? "(deleted bot)";
  const send = () =>
    request("send_room", { projectId: project.id, text }).then(() => (setText(""), setError("")), (e: Error) => setError(e.message));

  return (
    <section className="room">
      <h2>Room of {project.name}</h2>
      <ol>
        {room.map((l) => (
          <li key={l.seq}>
            <strong>{l.from ? name(l.from) : "You"}</strong>
            {l.to && <span className="note"> to {name(l.to)}</span>}
            <span className="note"> · {new Date(l.atMs).toLocaleTimeString()}</span>
            <div style={{ whiteSpace: "pre-wrap" }}>{l.text}</div>
          </li>
        ))}
      </ol>
      {room.length === 0 && <p className="note">Nothing said here yet.</p>}
      <form className="fields" onSubmit={(e) => { e.preventDefault(); if (text.trim()) send(); }}>
        <label>
          Message{" "}
          <textarea rows={3} value={text} onChange={(e) => setText(e.target.value)} />
        </label>
        <p className="note">
          {members.length
            ? `Start with @name to write to one bot (${members.map((b) => `@${b.name}`).join(", ")}). Without a name it goes to the project's lead. A bot that is busy gets it when it is free.`
            : "No bot is in this project yet. Add some on its Bots page."}
        </p>
        <button disabled={!text.trim()}>Send</button>
      </form>
      {error && <p className="error" role="alert">{error}</p>}
    </section>
  );
}
