/**
 * Text that gets a person's yes, with every character they cannot see put in plain view. A piece is
 * either plain text or one character shown as its code point (`U+200B`), with what kind of character
 * it is in `hidden`. Only the display changes: what runs is the text the service has, as it is.
 *
 * Not handled: a letter that only looks like another (a Cyrillic "а" for a Latin "a") is an ordinary,
 * visible character and is shown as it is. A grant is for the exact text, so a lookalike never
 * matches a command or path that was allowed before.
 */
export type Piece = { text: string; hidden?: string };

/** Each kind of character that is marked, by the ranges it covers (newline and tab are not among them). */
const KINDS: [name: string, ranges: string][] = [
  ["control character", "\\u0000-\\u0008\\u000B-\\u001F\\u007F-\\u009F"],
  ["zero-width or joining character", "\\u034F\\u180B-\\u180E\\u200B-\\u200D\\u2060-\\u2064\\uFE00-\\uFE0F\\uFEFF\\u{E0100}-\\u{E01EF}"],
  ["character that changes the reading order", "\\u061C\\u200E\\u200F\\u202A-\\u202E\\u2066-\\u2069"],
  ["space that is not an ordinary space", "\\u00A0\\u1680\\u2000-\\u200A\\u2028\\u2029\\u202F\\u205F\\u3000\\u2800"],
  ["blank letter or soft hyphen", "\\u00AD\\u115F\\u1160\\u17B4\\u17B5\\u3164\\uFFA0\\u2065\\u206A-\\u206F\\uFFF9-\\uFFFC"],
  ["tag character", "\\u{E0000}-\\u{E007F}"],
];
const HIDDEN = new RegExp(`[${KINDS.map(([, r]) => r).join("")}]`, "u");
const OF_KIND = KINDS.map(([name, ranges]) => [name, new RegExp(`[${ranges}]`, "u")] as const);

/** `text` as pieces; the pieces that are not marked, put together, are the text without them. */
export function reveal(text: string): Piece[] {
  const pieces: Piece[] = [];
  let plain = "";
  for (const ch of text) {
    if (!HIDDEN.test(ch)) {
      plain += ch;
      continue;
    }
    if (plain) pieces.push({ text: plain });
    plain = "";
    const code = ch.codePointAt(0)!.toString(16).toUpperCase().padStart(4, "0");
    pieces.push({ text: `U+${code}`, hidden: OF_KIND.find(([, re]) => re.test(ch))![0] });
  }
  if (plain) pieces.push({ text: plain });
  return pieces;
}

/** Whether any of `texts` holds a character that is marked. */
export const hasHidden = (...texts: (string | undefined)[]) => texts.some((t) => t !== undefined && HIDDEN.test(t));
