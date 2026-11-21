import { type ReactNode, useEffect, useState } from "react";
import { byLine, matches } from "./addons.ts";
import { request } from "./service.ts";

/** The catalog of an Add-ons tab: a list in Bate's repository on GitHub, read only when asked (reading it contacts GitHub), with a search and one button on each row. */
export function Catalog<T extends { name: string; description: string; author?: string }>(p: {
  kind: "workplaces" | "connectors" | "skills";
  /** What the list says when it has no entry. */
  empty?: string;
  /** The list is read again when this changes (what is installed says which entries are installed). */
  stamp?: string;
  /** The small lines of a row under its name. */
  lines: (e: T) => ReactNode;
  /** The button of a row; none: the row has no button. */
  action: (e: T) => { label: string; run: () => void } | undefined;
  /** Where a failure to read shows; "" clears it. */
  report: (message: string) => void;
}) {
  const [entries, setEntries] = useState<T[]>();
  const [query, setQuery] = useState("");
  const [reading, setReading] = useState(false);
  const load = () => {
    setReading(true);
    return request<{ catalog: T[] }>("list_catalog", { kind: p.kind })
      .then((d) => (setEntries(d.catalog), p.report("")), (e: Error) => p.report(e.message))
      .finally(() => setReading(false));
  };
  useEffect(() => {
    if (entries) void load();
  }, [p.stamp]);
  const shown = entries?.filter((e) => matches(query, e.name, e.description));

  return (
    <>
      <div className="head">
        <h2>Catalog</h2>
        <button className="small" disabled={reading} onClick={() => void load()}>{entries ? "Reload the catalog" : "Show the catalog"}</button>
      </div>
      {!entries && <p className="note">A list in Bate's repository on GitHub. Reading it contacts GitHub, so it waits until you ask.</p>}
      {entries?.length === 0 && <p className="note">{p.empty ?? "The catalog is empty."}</p>}
      {entries && entries.length > 0 && (
        <div className="find">
          <input type="search" aria-label="Search the catalog" placeholder="Search the catalog" value={query} onChange={(e) => setQuery(e.target.value)} />
        </div>
      )}
      <ul className="rows">
        {shown?.map((e) => {
          const action = p.action(e);
          return (
            <li key={e.name}>
              <div className="what">
                <strong>{e.name}</strong>
                {p.lines(e)}
                <small>{e.description}</small>
                {e.author && <small>{byLine(e.author)}</small>}
              </div>
              {action && (
                <div className="actions">
                  <button className="small" onClick={action.run}>{action.label}</button>
                </div>
              )}
            </li>
          );
        })}
      </ul>
      {entries && entries.length > 0 && query && shown?.length === 0 && <p className="note">Nothing in the catalog matches.</p>}
    </>
  );
}
