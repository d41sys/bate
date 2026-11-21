import { useEffect, useRef, useState } from "react";
import { Group, Problem, Setting, useSaves } from "./Form.tsx";
import { display, type QuickSettings as Settings, shortcutOf } from "./quick.ts";
import { listen, request, setQuickShortcut } from "./service.ts";

/** Quick Ask: the shortcut that opens its panel, and the two things it may take from the screen (PROTOCOL.md, "Quick Ask"). */
export function QuickSettings() {
  const [quick, setQuick] = useState<Settings>();
  const [installed, setInstalled] = useState<boolean>();
  const [recording, setRecording] = useState(false);
  const { run, fail, error, saved } = useSaves();
  const registered = useRef(false);
  const load = () =>
    Promise.all([request<Settings>("get_quick_ask"), request<{ installed: boolean }>("get_helper")]).then(([q, h]) => {
      setQuick(q);
      setInstalled(h.installed);
      // Once per page: the shell has the shortcut from its own start; asking again shows a key another app took since.
      if (!registered.current) {
        registered.current = true;
        setQuickShortcut(q.shortcut).catch((e: Error) => fail("shortcut", e.message));
      }
    }, (e: Error) => fail("load", e.message));
  useEffect(() => {
    void load();
    return listen((frame) => (frame.type === "quick_ask_changed" || frame.type === "helper_changed") && void load());
  }, []);
  // The system takes the key first: a shortcut another app holds is refused there, and is not saved here.
  const setShortcut = (shortcut: string | null) => run("shortcut", setQuickShortcut(shortcut).then(() => request("set_quick_ask", { shortcut })).then(load));
  const switches = [
    { key: "selection" as const, title: "Use selected text", hint: "The text selected in the front app goes with your question as a quote. macOS asks for Accessibility the first time." },
    { key: "screenshot" as const, title: "Attach a screenshot", hint: "The screen, or a window you pick, goes with your question as a picture. macOS asks for Screen Recording the first time." },
  ];
  return (
    <Group id="settings-quick" title="Quick Ask">
      {!quick && !error("load") && <p className="note">Loading…</p>}
      <Problem text={error("load")} />
      {quick && (
        <>
          <Setting inline title="Shortcut" hint="Opens a small panel over any app, to ask a bot at once. Off until you set one. It needs Ctrl, Alt or Cmd." error={error("shortcut")} saved={saved("shortcut")}>
            <>
              <button
                className={recording ? "primary" : undefined}
                aria-label="Shortcut"
                autoFocus={recording}
                onClick={() => setRecording(true)}
                onBlur={() => setRecording(false)}
                onKeyDown={(e) => {
                  if (!recording || e.nativeEvent.isComposing) return;
                  const shortcut = e.key === "Escape" ? undefined : shortcutOf(e);
                  if (e.key === "Escape") setRecording(false);
                  if (e.key === "Escape" || e.key === "Tab" || e.key === "Shift" || e.key === "Control" || e.key === "Alt" || e.key === "Meta") return;
                  e.preventDefault();
                  if (!shortcut) return;
                  setRecording(false);
                  void setShortcut(shortcut);
                }}
              >
                {recording ? "Press the keys…" : quick.shortcut ? display(quick.shortcut) : "Set a shortcut"}
              </button>
              {quick.shortcut && <button onClick={() => void setShortcut(null)}>Clear</button>}
            </>
          </Setting>
          {switches.map(({ key, title, hint }) => (
            <Setting
              key={key}
              title={title}
              hint={installed === false ? <>Needs Bate Helper: see “Bate Helper” above. {hint}</> : hint}
              error={error(key)}
            >
              <input
                type="checkbox"
                role="switch"
                aria-label={title}
                checked={quick[key]}
                disabled={installed === false && !quick[key]}
                onChange={(e) => void run(key, request("set_quick_ask", { [key]: e.target.checked }).then(load))}
              />
            </Setting>
          ))}
        </>
      )}
    </Group>
  );
}
