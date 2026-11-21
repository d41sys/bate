import type { Question } from "./chat.ts";

/** The service's choices, in the person's words. */
const CHOICES: Record<string, string> = { Approve: "Allow once", Always: "Always", Deny: "Deny" };

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
 * says the first card of a chat, whose Esc and ⌘Enter do the same as Deny and Allow once.
 */
export function QuestionCard({ q, keys, answer }: { q: Question; keys: boolean; answer: (choice: string) => void }) {
  return (
    <div className="question" role="group" aria-label={q.title}>
      <p>{q.title}</p>
      {q.target && <pre>{q.target}</pre>}
      {q.folder && <p className="note">In <code>{q.folder}</code></p>}
      {q.askedBy?.length ? <p className="note">Asked for by {q.askedBy.join(", who was asked by ")}</p> : null}
      <div className="actions">
        {q.options.map((choice) => (
          <button
            key={choice}
            className={choice === "Approve" ? "primary" : choice === "Deny" ? "danger" : undefined}
            // The keys are tips; what Always covers is the tip of its button.
            data-tip={choice === "Approve" ? (keys ? "Allow once (⌘Enter)" : undefined) : choice === "Deny" ? (keys ? "Deny (Esc)" : undefined) : `Bate stops asking for ${COVERS[q.kind ?? ""] ?? "this exact action"}. The bot still asks again for anything else. Remove it in Settings.`}
            onClick={() => answer(choice)}
          >
            {choice === "Always" ? <>Always allow <code>{q.target ? short(q.target) : "this action"}</code></> : CHOICES[choice] ?? choice}
          </button>
        ))}
      </div>
    </div>
  );
}
