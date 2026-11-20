/** What the service says of the model server (`get_provider`). */
export type Provider = { url: string | null; hasKey: boolean; fromEnv: boolean };

/** The page remembers "Set up later" here: the first-run screen is not shown at every start. */
export const LATER = "bate.first-run.later";

/**
 * Whether no provider is set yet: neither ChatGPT nor a model server. `fromEnv` does not count: the service
 * sets it whenever no server is saved, with or without an address in its environment (that address is `url`).
 */
export const needsProvider = (chatgpt: boolean, provider: Provider) => !chatgpt && !provider.url;

/** What a bot is made from: the form's start (a card fills it in), and the bot signing in makes. */
export type Starter = { name: string; purpose: string; avatar: string };

/** The first bot, made when the person signs in, and the message the box starts with. */
export const FIRST_BOT: Starter & { message: string } = {
  name: "Scout",
  purpose: "Answers questions and works on the files in its folder.",
  avatar: "🧭",
  message: "Say hello, then tell me three things you can do for me here.",
};

export const STARTERS: Starter[] = [
  { name: "Researcher", purpose: "Finds things out and reports what it found, with where each fact came from.", avatar: "🔎" },
  { name: "Writer", purpose: "Drafts and edits text, and asks what it needs to know before it writes.", avatar: "✍️" },
  { name: "Coder", purpose: "Reads and changes the code in its folder, runs the tests and says what changed.", avatar: "💻" },
  { name: "Organiser", purpose: "Keeps notes and lists in its folder, tidy and easy to find again.", avatar: "🗂️" },
];
