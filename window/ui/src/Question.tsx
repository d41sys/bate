import { useState } from "react";
import type { Question } from "./chat.ts";
import { Revealed } from "./Reveal.tsx";
import { hasHidden } from "./reveal.ts";
import { inShell } from "./service.ts";

/** The service's choices, in the person's words. */
const CHOICES: Record<string, string> = { Approve: "Allow once", Conversation: "Allow in this conversation", Always: "Always", Deny: "Deny" };

/** What "Always" remembers: a grant is for this kind of action and exactly this target (PROTOCOL.md, Permissions). */
const COVERS: Record<string, string> = {
  read: "this exact path",
  write: "this exact file",
  run: "this exact command",
  connector: "this connector tool, whatever it is given",
};

/** The first line of a target, cut to what a button can hold; the card shows all of it above. */
const short = (target: string) => {
  const line = target.split("\n")[0];
  return line.length > 32 ? `${line.slice(0, 31)}…` : line;
};

/**
 * What a bot asks, with the choices the service offered: no "Always" where it did not offer one. `keys`
 * says the first card of a chat, whose Esc and ⌘Enter do the same as Deny and Allow once. The title and
 * the target are all of what will run, and what a person cannot see in them is marked.
 */
export function QuestionCard({ q, keys, answer }: { q: Question; keys: boolean; answer: (choice: string, secret?: string) => void }) {
  if (q.secret) return <SecretCard q={q} secret={q.secret} keys={keys} answer={answer} />;
  const covers = COVERS[q.kind ?? ""] ?? "this exact action";
  const tip = (choice: string) =>
    choice === "Approve" ? (keys ? "Allow once (⌘Enter)" : undefined)
    : choice === "Deny" ? (keys ? "Deny (Esc)" : undefined)
    : choice === "Conversation" ? `Bate stops asking for ${covers} in this conversation only. Another conversation, a fork and a routine's run still ask. Remove it in Settings.`
    // The keys are tips; what Always covers is the tip of its button.
    : `Bate stops asking for ${covers}. The bot still asks again for anything else. Remove it in Settings.`;
  return (
    <div className="question" role="group" aria-label={q.title}>
      <p><Revealed text={q.title} /></p>
      {q.target && <pre><Revealed text={q.target} /></pre>}
      {hasHidden(q.title, q.target) && <p className="warning">Some characters here cannot be seen or change the order of the text. They are marked, like <mark className="hidden-char">U+200B</mark>. Check what you allow.</p>}
      {q.folder && <p className="note">In <code>{q.folder}</code></p>}
      {q.askedBy?.length ? <p className="note">Asked for by {q.askedBy.join(", who was asked by ")}</p> : null}
      <div className="actions">
        {q.options.map((choice) => (
          <button
            key={choice}
            className={choice === "Approve" ? "primary" : choice === "Deny" ? "danger" : undefined}
            data-tip={tip(choice)}
            onClick={() => answer(choice)}
          >
            {choice === "Always" ? <>Always allow <code>{q.target ? <Revealed text={short(q.target)} /> : "this action"}</code></> : CHOICES[choice] ?? choice}
          </button>
        ))}
      </div>
    </div>
  );
}

/**
 * A bot asks for a secret for one site. The value is typed here, hidden, and goes to the service in
 * `answer_secret`; it is not kept in the page after that. Only Bate's own window takes it: a device on the
 * network can refuse, not type.
 */
function SecretCard({ q, secret, keys, answer }: { q: Question; secret: { name: string; site: string }; keys: boolean; answer: (choice: string, secret?: string) => void }) {
  const [value, setValue] = useState("");
  const save = () => {
    if (!value) return;
    answer("", value);
    setValue("");
  };
  return (
    <div className="question" role="group" aria-label={q.title}>
      <p><Revealed text={q.title} /></p>
      {hasHidden(q.title) && <p className="warning">Some characters here cannot be seen or change the order of the text. They are marked, like <mark className="hidden-char">U+200B</mark>.</p>}
      <p className="note">For <strong>{secret.site}</strong>, saved as <code>{secret.name}</code>. The bot never sees it: Bate types it only on that site.</p>
      {inShell ? (
        <input
          type="password"
          autoComplete="off"
          aria-label={`Secret for ${secret.site}`}
          value={value}
          onChange={(e) => setValue(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && !e.nativeEvent.isComposing && save()}
        />
      ) : (
        <p className="note">Type it in Bate's own window on the Mac. Here you can only deny it.</p>
      )}
      <div className="actions">
        {inShell && <button className="primary" disabled={!value} onClick={save}>Save</button>}
        <button className="danger" data-tip={keys ? "Deny (Esc)" : undefined} onClick={() => answer("Deny")}>Deny</button>
      </div>
    </div>
  );
}
