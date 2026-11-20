/**
 * The rows of the model list for what is typed in its search box: the models that contain it.
 * `listed` is what the server offers; when it offers none, the page suggests the models other bots
 * use (`seen`) and the typed name itself is one more row (`typed`), because any name may be right.
 * The bot's own model (`current`) is always among the candidates.
 */
export function modelChoices(listed: string[], seen: string[], current: string, query: string): { models: string[]; typed?: string } {
  const free = listed.length === 0;
  const all = [...new Set([...(free ? seen : listed), ...(current ? [current] : [])])];
  const q = query.trim();
  const models = all.filter((m) => m.toLowerCase().includes(q.toLowerCase()));
  return { models, typed: free && q && !all.includes(q) ? q : undefined };
}
