/** What the service says of the model server (`get_provider`). */
export type Provider = { url: string | null; hasKey: boolean; fromEnv: boolean };

/** The page remembers a dismissal here: a first run is shown once, not at every start. */
export const DISMISSED = "bate.first-run.dismissed";

/** Whether no provider is set yet: neither ChatGPT nor a model server. */
export const needsProvider = (chatgpt: boolean, provider: Provider) => !chatgpt && !provider.url && !provider.fromEnv;
