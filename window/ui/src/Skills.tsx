import { useState } from "react";
import { each, madeBy, onFor, placed } from "./addons.ts";
import { Choose } from "./ProjectDialog.tsx";
import { RowMenu } from "./RowMenu.tsx";
import { Sheet } from "./Sheet.tsx";
import { usePerBot } from "./usePerBot.ts";
import { inShell, request } from "./service.ts";
import type { Bot, InstalledSkill, SkillPreview } from "./types.ts";
import { sizeText } from "./workplaces.ts";

/** Where a skill is copied from: a folder, or the same skill on a bot that has it. */
type Source = { path: string } | { fromBot: string; name: string };
type Ask = { preview: SkillPreview; source: Source; maker: string } | { remove: string };

/** The skills of every bot in one list: what each is, who has it, and one install for as many bots as chosen. */
export function Skills({ bots }: { bots: Bot[] }) {
  const { lists, error: loadError } = usePerBot<InstalledSkill>(bots, "list_skills", "skills", "skills_changed");
  const rows = placed(lists);
  const [ask, setAsk] = useState<Ask>();
  const [error, setError] = useState("");
  const names = (ids: string[]) => ids.map((id) => bots.find((b) => b.id === id)?.name ?? id).join(", ");
  const has = (name: string) => rows.find((r) => r.name === name)?.users.map((u) => u.bot) ?? [];

  const switchTo = (bot: string, name: string, enabled: boolean) =>
    request("set_skill_enabled", { bot, name, enabled }).then(() => setError(""), (e: Error) => setError(e.message));

  const look = (source: Source, maker: string) =>
    request<SkillPreview>("preview_skill", source).then((preview) => (setAsk({ preview, source, maker }), setError("")), (e: Error) => setError(e.message));

  return (
    <section>
      <div className="head">
        <h2>Installed</h2>
        <Choose folder set={(path) => void look({ path }, madeBy.folder)} fail={setError} />
      </div>
      {rows.length === 0 && <p className="note">No skills yet.{inShell ? " Choose a folder with a SKILL.md in it." : ""}</p>}
      <ul className="rows">
        {rows.map((r) => (
          <li key={r.name}>
            <div className="what">
              <strong>{r.name}</strong>
              <small>{r.users[0].item.description}</small>
            </div>
            <div className="actions">
              <RowMenu
                label={r.name}
                items={[
                  ...(inShell ? [{ label: "Add to bots…", run: () => void look({ fromBot: r.users[0].bot, name: r.name }, madeBy.copy(names([r.users[0].bot]))) }] : []),
                  { label: "Remove…", danger: true, run: () => setAsk({ remove: r.name }) },
                ]}
              />
            </div>
            <details>
              <summary>{onFor(r.users.map((u) => u.item), bots.length)}</summary>
              <ul className="rows">
                {r.users.map(({ bot, item }) => (
                  <li key={bot}>
                    <label>
                      <input type="checkbox" role="switch" checked={item.enabled} onChange={(e) => void switchTo(bot, r.name, e.target.checked)} />
                      {names([bot])}
                    </label>
                    <small>{item.enabled ? "On" : "Off: the bot is not told of it"}</small>
                  </li>
                ))}
              </ul>
            </details>
          </li>
        ))}
      </ul>
      {!inShell && <p className="note">Skills are added in Bate's own window on the Mac.</p>}
      {(error || loadError) && <p className="error" role="alert">{error || loadError}</p>}
      {ask && "preview" in ask && (
        <Sheet
          title={`Install ${ask.preview.skill.name}`}
          about={ask.preview.skill.description}
          maker={ask.maker}
          facts={[`${ask.preview.entries} ${ask.preview.entries === 1 ? "file or folder" : "files and folders"}, ${sizeText(ask.preview.bytes)}`, "Instructions in SKILL.md, and the files they refer to."]}
          may={[
            "It does nothing by itself: the bot reads it when a task fits, and may use the files in it.",
            "Each command or file access the bot makes for it is asked about under that bot's permissions.",
          ]}
          who={{
            legend: "Install for",
            items: bots.map((b) => ({ id: b.id, bot: b, label: b.name, note: has(ask.preview.skill.name).includes(b.id) ? "Has it already: this replaces it" : undefined })),
          }}
          action="Install"
          go={(ids) => each(ids, (bot) => request("install_skill", { bot, ...ask.source }))}
          close={() => setAsk(undefined)}
        />
      )}
      {ask && "remove" in ask && (
        <Sheet
          title={`Remove ${ask.remove}`}
          about={`This deletes the folder skills/${ask.remove} and everything in it from each bot ticked, including files the bot added there. The folder it was installed from is not touched.`}
          who={{ legend: "Remove from", items: bots.filter((b) => has(ask.remove).includes(b.id)).map((b) => ({ id: b.id, bot: b, label: b.name })), ticked: has(ask.remove) }}
          action="Remove"
          danger
          go={(ids) => each(ids, (bot) => request("delete_skill", { bot, name: ask.remove }))}
          close={() => setAsk(undefined)}
        />
      )}
    </section>
  );
}
