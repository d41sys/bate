import type { Question } from "./chat.ts";
import { Revealed } from "./Reveal.tsx";
import { hasHidden } from "./reveal.ts";

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
export function QuestionCard({ q, keys, answer }: { q: Question; keys: boolean; answer: (choice: string) => void }) {
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
