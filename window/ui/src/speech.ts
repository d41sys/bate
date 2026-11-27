// Read-aloud with the web view's own speech (`speechSynthesis`): on a Mac the system's voices,
// which speak with no network. Another device's browser may offer only voices that do; those
// are not used.

/** A voice that can speak here: the browser may not have one. */
export const canRead = "speechSynthesis" in window;

/** What a reply says, as the page shows it: no Markdown marks, no link addresses, no code blocks. */
export const readable = (reply: HTMLElement) =>
  Array.from(reply.children)
    .filter((block) => !block.classList.contains("code"))
    .map((block) => (block as HTMLElement).innerText)
    .join("\n")
    .trim();

/** Speaks `text`, after stopping what is being spoken; `done` runs when it ends or is stopped. */
export function speak(text: string, done: () => void) {
  speechSynthesis.cancel();
  // `getVoices` is empty until the web view has loaded them: then its default voice is used,
  // and that one is local.
  const voices = speechSynthesis.getVoices();
  const local = voices.filter((v) => v.localService);
  if (voices.length && !local.length) throw new Error("No voice that works offline is installed");
  const said = new SpeechSynthesisUtterance(text);
  // The default voice, unless a browser's default is one that needs the network.
  if (voices.find((v) => v.default)?.localService === false) said.voice = local[0];
  // shortcut: the voice is the default one whatever language the reply is in; upgrade by
  // choosing a voice for the reply's language.
  said.onend = said.onerror = done;
  speechSynthesis.speak(said);
}

export const hush = () => speechSynthesis.cancel();
