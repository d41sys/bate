import { type CSSProperties, useEffect, useRef, useState } from "react";
import { listen, request } from "./service.ts";
import { PART, clampOffset, nextOffset, prevOffset, range, visibleNodes } from "./surface.ts";
import type { Surface, SurfacePart } from "./types.ts";

const KINDS = { table: "Table", tree: "Tree", code: "Code" } as const;
/** A cell of more characters than this wraps; a shorter one stays on its line. About what `--control-w` holds. */
const LONG = 32;

/** The surfaces a project's program offers, a row each, to open one in the pane (`current`: the one open). */
export function Surfaces({ projectId, current, open, workplaces }: {
  projectId: string;
  current?: string;
  open: (s: Surface) => void;
  workplaces: () => void;
}) {
  const [list, setList] = useState<Surface[]>();
  const [error, setError] = useState("");
  // Bumped to ask again after a failure.
  const [again, setAgain] = useState(0);
  useEffect(() => {
    let live = true;
    const load = () =>
      request<{ surfaces: Surface[] }>("list_surfaces", { projectId }).then(
        (d) => {
          if (!live) return;
          setError("");
          setList(d.surfaces);
        },
        (e: Error) => live && setError(e.message),
      );
    load();
    const off = listen((f) => {
      if ((f.type === "surfaces_changed" && f.projectId === projectId) || f.type === "projects_changed") load();
    });
    return () => {
      live = false;
      off();
    };
  }, [projectId, again]);
  if (error)
    return (
      <div className="empty">
        <p className="error" role="alert">{error}</p>
        <button type="button" onClick={() => setAgain((n) => n + 1)}>Try again</button>
      </div>
    );
  if (!list) return <p className="note">Loading…</p>;
  if (!list.length)
    return (
      <div className="empty">
        <p>No surfaces yet. A workplace's program fills them.</p>
        <button type="button" onClick={workplaces}>See workplaces</button>
      </div>
    );
  return (
    <ul className="surfaces" aria-label="Surfaces of the project">
      {list.map((s) => (
        <li key={s.uri}>
          <button type="button" className={s.uri === current ? "row current" : "row"} aria-pressed={s.uri === current} onClick={() => open(s)}>
            <span className="label">
              <span>{s.name}</span>
              <small>{s.description ? `${KINDS[s.kind]}: ${s.description}` : KINDS[s.kind]}</small>
            </span>
          </button>
        </li>
      ))}
    </ul>
  );
}

/**
 * One surface, drawn here: a table, a tree or a code view, one part at a time. What the
 * program sends is only ever put in as text. Reads again when the program says it changed.
 */
export function SurfaceView({ projectId, uri }: { projectId: string; uri: string }) {
  const [offset, setOffset] = useState(0);
  const [part, setPart] = useState<SurfacePart>();
  const [error, setError] = useState("");
  const [result, setResult] = useState<{ ok: boolean; text: string }>();
  const [selected, setSelected] = useState<string>();
  const [collapsed, setCollapsed] = useState<ReadonlySet<string>>(new Set());
  // Bumped to read again; `latest` drops an answer that a newer read has overtaken.
  const [again, setAgain] = useState(0);
  const latest = useRef(0);

  useEffect(() => {
    let live = true;
    const load = () => {
      const mine = ++latest.current;
      request<SurfacePart>("read_surface", { projectId, uri, offset, limit: PART }).then(
        (p) => {
          if (!live || mine !== latest.current) return;
          setError("");
          const at = clampOffset(p);
          if (at !== offset) setOffset(at);
          else setPart(p);
        },
        (e: Error) => live && mine === latest.current && setError(e.message),
      );
    };
    load();
    const off = listen((f) => {
      const mine = f.type === "surface_changed" && f.projectId === projectId && f.uri === uri;
      // The program started again, or the service did: what was open is read anew.
      if (mine || f.type === "projects_changed" || f.type === "connected") load();
    });
    return () => {
      live = false;
      off();
    };
  }, [projectId, uri, offset, again]);

  /** A tool of the program, with what was chosen (and written); the surface is read again for the result. */
  const act = (tool: string, target?: string, text?: string) =>
    request<{ ok: boolean; text: string }>("act_surface", { projectId, uri, tool, target, text }).then(
      (r) => {
        setResult({ ok: r.ok, text: r.text || (!r.ok ? "That did not work." : text === undefined ? "Done." : "Saved.") });
        setAgain((n) => n + 1);
      },
      (e: Error) => setResult({ ok: false, text: e.message }),
    );
  const choose = (id: string) => {
    setSelected(id);
    if (part?.select) void act(part.select, id);
  };
  // Enter or a double click opens the item.
  const opener = (id: string) =>
    part?.open
      ? {
          onDoubleClick: () => void act(part.open!, id),
          onKeyDown: (e: React.KeyboardEvent) => {
            if (e.key === "Enter") void act(part.open!, id);
            if (e.key === " ") {
              e.preventDefault();
              choose(id);
            }
          },
        }
      : {};
  const pickable = part?.select || part?.open;

  if (!part)
    return error ? (
      <div className="empty">
        <p className="error" role="alert">{error}</p>
        <button type="button" onClick={() => setAgain((n) => n + 1)}>Try again</button>
      </div>
    ) : (
      <p className="note">Loading…</p>
    );
  const move = (to: number | undefined) => to !== undefined && setOffset(to);
  return (
    <div className="surface">
      <div className="tools">
        {part.actions.map((a) =>
          a.input ? (
            <form
              className="find"
              key={a.tool}
              onSubmit={(e) => {
                e.preventDefault();
                const field = e.currentTarget.elements.namedItem("text") as HTMLInputElement;
                void act(a.tool, selected, field.value);
                field.value = "";
              }}
            >
              <input name="text" aria-label={a.label} placeholder={a.label} maxLength={2000} />
              <button type="submit">{a.label}</button>
            </form>
          ) : (
            <button key={a.tool} type="button" onClick={() => void act(a.tool, selected)}>{a.label}</button>
          ),
        )}
        {(prevOffset(part) !== undefined || nextOffset(part) !== undefined) && (
          <div className="pager">
            <small>{range(part)}</small>
            <button type="button" className="small" disabled={prevOffset(part) === undefined} onClick={() => move(prevOffset(part))}>Previous</button>
            <button type="button" className="small" disabled={nextOffset(part) === undefined} onClick={() => move(nextOffset(part))}>Next</button>
          </div>
        )}
      </div>
      {error && <p className="error" role="alert">{error}</p>}
      {result && <p className={result.ok ? "note" : "error"} role="status">{result.text}</p>}
      <div className="body">
        {part.total === 0 && (
          <div className="empty">
            <p>Nothing here yet.</p>
            <button type="button" className="small" onClick={() => setAgain((n) => n + 1)}>Read again</button>
          </div>
        )}
        {part.total > 0 && part.kind === "table" && (
          <table>
            <thead>
              <tr>{part.columns.map((c, i) => <th key={i}>{c}</th>)}</tr>
            </thead>
            <tbody>
              {part.rows.map((r, i) => (
                <tr
                  key={part.offset + i}
                  tabIndex={pickable ? 0 : undefined}
                  aria-selected={pickable ? r.id === selected : undefined}
                  onClick={pickable ? () => choose(r.id) : undefined}
                  {...opener(r.id)}
                >
                  {r.cells.map((c, j) => <td key={j} className={c.length > LONG ? "long" : undefined}>{c}</td>)}
                </tr>
              ))}
            </tbody>
          </table>
        )}
        {part.total > 0 && part.kind === "tree" && (
          <ul role="tree" className="tree">
            {visibleNodes(part.nodes, collapsed).map(({ index, hasChildren }) => {
              const n = part.nodes[index];
              return (
                <li
                  key={part.offset + index}
                  role="treeitem"
                  aria-level={n.depth + 1}
                  aria-expanded={hasChildren ? !collapsed.has(n.id) : undefined}
                  aria-selected={pickable ? n.id === selected : undefined}
                  tabIndex={pickable ? 0 : undefined}
                  style={{ "--depth": n.depth } as CSSProperties}
                  onClick={pickable ? () => choose(n.id) : undefined}
                  {...opener(n.id)}
                >
                  {hasChildren ? (
                    <button
                      type="button"
                      className="ghost small"
                      aria-label={`${collapsed.has(n.id) ? "Expand" : "Collapse"} ${n.label}`}
                      onClick={(e) => {
                        e.stopPropagation();
                        setCollapsed((c) => {
                          const next = new Set(c);
                          if (!next.delete(n.id)) next.add(n.id);
                          return next;
                        });
                      }}
                    >
                      {collapsed.has(n.id) ? "▸" : "▾"}
                    </button>
                  ) : (
                    <span className="gap" aria-hidden="true" />
                  )}
                  {n.label}
                </li>
              );
            })}
          </ul>
        )}
        {part.total > 0 && part.kind === "code" && (
          <pre className="code" aria-label={part.language ? `Code, ${part.language}` : "Code"}>
            {part.lines.map((line, i) => {
              // A line is chosen by its number, as a person reads it.
              const no = String(part.offset + i + 1);
              return (
                <div
                  key={part.offset + i}
                  className="line"
                  tabIndex={pickable ? 0 : undefined}
                  aria-selected={pickable ? no === selected : undefined}
                  onClick={pickable ? () => choose(no) : undefined}
                  {...opener(no)}
                >
                  <span className="no">{no}</span>
                  {line}
                </div>
              );
            })}
          </pre>
        )}
      </div>
    </div>
  );
}
