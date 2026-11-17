Method A (the Codex client id) is the common one: 10 of 11 apps implement it (the 11th, NanoClaw, runs the Codex CLI, which is A); only OpenClaw, opencode and pi also implement Method B, the same method as Bate, and none uses B alone.

# How other apps sign in with a ChatGPT subscription

Surveyed 2026-11-17 from shallow clones of each default branch. Links are permalinks at the commit named in each section. Method A = reuse the Codex CLI client id and call `chatgpt.com/backend-api/codex`. Method B = OpenAI's documented "Sign in with ChatGPT" for open-source apps (`dynamic_agent_client`, then a per-user `oaiapp_...` id, calls `api.openai.com/v1`).

## Count per method

| Method | Apps |
|---|---|
| A only | Hermes Agent, nanobot, ZeroClaw, PicoClaw, Cline, Roo Code, Goose (7) |
| A and B | OpenClaw, opencode, pi (3) |
| B only | none (0) |
| No own sign-in; runs `codex login` (A) | NanoClaw (1) |
| Reference for A | OpenAI Codex CLI |

Bate is B only. No surveyed app is B only.

## Table

A-rows: authorize URL is `https://auth.openai.com/oauth/authorize`, token URL is `https://auth.openai.com/oauth/token`. B-rows: authorize URL is `https://auth.openai.com/api/accounts/authorize`, token URL is `https://auth.openai.com/api/accounts/oauth/token`.

| App | Has ChatGPT sign-in | Method | Client id | Authorize URL | API base | Model list source | Source link |
|---|---|---|---|---|---|---|---|
| OpenClaw (login) | Yes | A | `app_EMoamEEZ73f0CkXaXp7hrann` | `/oauth/authorize` | `chatgpt.com/backend-api/codex` | not verified | [openai-chatgpt-oauth-authorization.runtime.ts#L3-L4](https://github.com/openclaw/openclaw/blob/c53f0575bdac2546b35604712363993d27047b8e/extensions/openai/openai-chatgpt-oauth-authorization.runtime.ts#L3-L4) |
| OpenClaw (SIWC) | Yes | B | `dynamic_agent_client` | `/api/accounts/authorize` | `api.openai.com/v1` | `GET api.openai.com/v1/models`, keep `visibility == "list"` | [token-sharing.ts#L16](https://github.com/openclaw/openclaw/blob/c53f0575bdac2546b35604712363993d27047b8e/extensions/openai/token-sharing.ts#L16) |
| Hermes Agent | Yes | A | `app_EMoamEEZ73f0CkXaXp7hrann` | `/oauth/authorize` (browser) or device-code | `chatgpt.com/backend-api/codex` | `GET chatgpt.com/backend-api/codex/models` | [auth_constants.py#L90](https://github.com/NousResearch/hermes-agent/blob/b076b5856cc6b74ab815ea97d866ee5022a5282a/hermes_cli/auth_constants.py#L90) |
| nanobot | Yes | A (via `oauth-cli-kit`) | `app_EMoamEEZ73f0CkXaXp7hrann` | `/oauth/authorize` | `chatgpt.com/backend-api/codex/responses` | `GET chatgpt.com/backend-api/codex/models`, bundled fallback | [openai_codex.py#L10-L13 in oauth-cli-kit 0.1.6](https://files.pythonhosted.org/packages/7d/65/db8768844cff218ae2bb19305ec1abe95e414c9953af205fe1b75a5e2300/oauth_cli_kit-0.1.6.tar.gz) |
| ZeroClaw | Yes | A | `app_EMoamEEZ73f0CkXaXp7hrann` | `/oauth/authorize` | `chatgpt.com/backend-api/codex/responses` | not verified in code; docs say curl `codex/models` | [openai_oauth.rs#L18-L22](https://github.com/zeroclaw-labs/zeroclaw/blob/cda933e6b9937deacbcc0d4d2cd7f1ae5bb4500e/crates/zeroclaw-providers/src/auth/openai_oauth.rs#L18-L22) |
| PicoClaw | Yes | A | `app_EMoamEEZ73f0CkXaXp7hrann` | `/oauth/authorize` | `chatgpt.com/backend-api/codex` | not verified | [oauth.go#L44-L48](https://github.com/sipeed/picoclaw/blob/bbf6893ca7afad27f1d00a0f5a45982a549c6ed6/pkg/auth/oauth.go#L44-L48) |
| NanoClaw | Indirect (runs `codex login`) | A (Codex CLI's own) | none in NanoClaw code | n/a | n/a (Codex app-server) | n/a | [codex.ts#L167](https://github.com/nanocoai/nanoclaw/blob/2cb6926bb66bba9472089e67e4a26dba0663cca8/.claude/skills/add-codex/payload/setup/providers/codex.ts#L167) |
| opencode (`chatgpt-browser`/`headless`) | Yes | A | `app_EMoamEEZ73f0CkXaXp7hrann` | `/oauth/authorize` | `chatgpt.com/backend-api/codex` | shared OpenAI catalog, filtered to Codex-eligible models | [openai.ts#L18](https://github.com/anomalyco/opencode/blob/b03809b2d79d48b7f70d45e9efd9519c4872583d/packages/core/src/plugin/provider/openai.ts#L18) |
| opencode (SIWC) | Yes | B | `dynamic_agent_client` | `/api/accounts/authorize` | `api.openai.com/v1` | `GET api.openai.com/v1/models`, keep `visibility == "list"` and `supported_in_api` | [chatgpt.ts#L23](https://github.com/anomalyco/opencode/blob/b03809b2d79d48b7f70d45e9efd9519c4872583d/packages/core/src/plugin/provider/chatgpt.ts#L23) |
| Cline | Yes | A | `app_EMoamEEZ73f0CkXaXp7hrann` | `/oauth/authorize` | `chatgpt.com/backend-api/codex` | bundled OpenAI catalog, filtered | [codex.ts#L29-L32](https://github.com/cline/cline/blob/eff43e19b267e171f597ab03733bfed25090f72d/sdk/packages/core/src/auth/codex.ts#L29-L32) |
| Roo Code (archived) | Yes | A | `app_EMoamEEZ73f0CkXaXp7hrann` | `/oauth/authorize` | `chatgpt.com/backend-api/codex` | bundled `openAiCodexModels` | [oauth.ts#L18-L23](https://github.com/RooCodeInc/Roo-Code/blob/b867ec9145750d0ae1ff7f02d35406e9bf2a0b16/src/integrations/openai-codex/oauth.ts#L18-L23) |
| Goose | Yes | A | `app_EMoamEEZ73f0CkXaXp7hrann` | `/oauth/authorize` | `chatgpt.com/backend-api/codex` | bundled `CHATGPT_CODEX_KNOWN_MODELS` | [chatgpt_codex.rs#L38-L40](https://github.com/aaif-goose/goose/blob/d8962e321eb00444d0cdfc5c73b242d05a25887c/crates/goose/src/providers/chatgpt_codex.rs#L38-L40) |
| pi (`openai-codex`) | Yes | A | `app_EMoamEEZ73f0CkXaXp7hrann` | `/oauth/authorize` | `chatgpt.com/backend-api` (+ `/codex/responses`) | bundled catalog | [openai-codex.ts#L22-L26](https://github.com/earendil-works/pi/blob/cead615b48b80d1c61eecab2f0b576d16eeaf4c5/packages/ai/src/auth/oauth/openai-codex.ts#L22-L26) |
| pi (`openai`, SIWC) | Yes | B | `dynamic_agent_client` | `/api/accounts/authorize` | `api.openai.com/v1` | bundled catalog (no `/v1/models` call found) | [openai-chatgpt.ts#L16-L25](https://github.com/earendil-works/pi/blob/cead615b48b80d1c61eecab2f0b576d16eeaf4c5/packages/ai/src/auth/oauth/openai-chatgpt.ts#L16-L25) |
| OpenAI Codex CLI | Yes (owner of the id) | A | `app_EMoamEEZ73f0CkXaXp7hrann` | `/oauth/authorize` | `chatgpt.com/backend-api/codex` | bundled `models.json` + cache + remote `/models` | [manager.rs#L1897](https://github.com/openai/codex/blob/8011b92bc82d4cc1b6dc3f35c0cd28768c797c97/codex-rs/login/src/auth/manager.rs#L1897) |

Callback URL, per app (all port 1455 unless noted):

| App | Callback |
|---|---|
| A-apps: Hermes (browser flow), nanobot, ZeroClaw, PicoClaw, Cline, Roo, Goose, pi, OpenClaw | `http://localhost:1455/auth/callback` |
| opencode (A) | `http://localhost:{port}/auth/callback`, port 1455 with fallback 1457 (`openai.ts:20-21,101`) |
| Codex CLI (A) | `http://127.0.0.1:1455/auth/callback` |
| pi (B), opencode (B) | `http://127.0.0.1:1455/auth/callback` (opencode: random free port) |
| OpenClaw (B) | `http://localhost:0/auth/callback`, port 0 = free port picked per attempt, listener bound to 127.0.0.1 |

## Evidence per app

### 1. OpenClaw (`openclaw/openclaw`, `c53f0575`)
Both methods. Own docs list them side by side.
- A: `extensions/openai/openai-chatgpt-oauth-authorization.runtime.ts:3` `const CLIENT_ID = "app_EMoamEEZ73f0CkXaXp7hrann";` `:4` `AUTHORIZE_URL = "https://auth.openai.com/oauth/authorize"`, `:5` `CALLBACK_PORT = 1455`. It also sends `codex_cli_simplified_flow=true` and `originator`. Token URL: `openai-chatgpt-oauth-token.runtime.ts:23`.
- B: `extensions/openai/token-sharing.ts:16` `export const TOKEN_SHARING_CLIENT_ID = "dynamic_agent_client";`, `:4` resource `https://api.openai.com/v1`, `:7` redirect `http://localhost:0/auth/callback`. `token-sharing-oauth.runtime.ts:352` authorize `.../api/accounts/authorize`, `:40` token `.../api/accounts/oauth/token`, `:363` `agent_name_hint: "OpenClaw"`.
- API base: `docs/providers/openai/authentication.md:41` "Default model endpoint | `https://api.openai.com/v1/responses` | `https://chatgpt.com/backend-api/codex/responses`" (SIWC column, then Codex login column).
- Models (B): `extensions/openai/token-sharing-catalog.ts:38` `endpoint: \`${TOKEN_SHARING_RESOURCE}/models\``, `:57` `if (record?.visibility !== "list" ...`.
- Default: `docs/providers/openai/authentication.md:184` "Without `--method`, OpenAI login defaults to Codex browser OAuth." (so A is the default.)
- Own docs on terms: no risk or terms warning found. `docs/concepts/oauth.md:189` says "OpenAI Codex OAuth is explicitly supported for use outside the Codex CLI, including OpenClaw workflows." This is OpenClaw's claim; it cites no OpenAI source.

### 2. Hermes Agent (`NousResearch/hermes-agent`, `b076b585`)
A only, in source. Default flow is device code; browser flow is opt-in.
- `hermes_cli/auth_constants.py:90` `CODEX_OAUTH_CLIENT_ID = "app_EMoamEEZ73f0CkXaXp7hrann"`, `:91` token URL `https://auth.openai.com/oauth/token`, `:74` `DEFAULT_CODEX_BASE_URL = "https://chatgpt.com/backend-api/codex"`.
- `hermes_cli/auth_codex_browser.py:33` `CODEX_OAUTH_AUTHORIZE_URL = "https://auth.openai.com/oauth/authorize"`, `:37` `CODEX_BROWSER_CALLBACK_PORT = 1455`, `:125` `redirect_uri = f"http://localhost:{...}..."`. Docstring `:5`: "the redirect URI fixed by the public Codex client registration".
- Device code: `hermes_cli/auth_codex.py:1029` POST `{issuer}/api/accounts/deviceauth/usercode`.
- Models: `agent/model_metadata.py:1760` `CODEX_MODELS_CATALOG_ENDPOINT = "https://chatgpt.com/backend-api/codex/models"`. Comment `:1845`: without the account id header it "returns `{"models":[]}`".
- No `dynamic_agent_client`, `ext_agent_host_id` or `chatgpt.tokens.use.direct` anywhere in the repo (searched with `rg`). Own docs: no terms or risk text found.
- Conflict to note: OpenAI's partner page lists "Hermes Agent" under "Apps with ChatGPT plan usage" (see "What OpenAI says"). Source shows only A. See "Not verified".

### 3. nanobot (`HKUDS/nanobot`, `53a55658`)
A, through the PyPI package `oauth-cli-kit`.
- `pyproject.toml:38` `"oauth-cli-kit>=0.1.6,<1.0.0"`; `nanobot/providers/openai_codex_provider.py:17` `from oauth_cli_kit import get_token as get_codex_token`.
- In the package (sdist 0.1.6) `oauth_cli_kit/providers/openai_codex.py:10-14`: `client_id="app_EMoamEEZ73f0CkXaXp7hrann"`, `authorize_url="https://auth.openai.com/oauth/authorize"`, `token_url="https://auth.openai.com/oauth/token"`, `redirect_uri="http://localhost:1455/auth/callback"`, `scope="openid profile email offline_access"`; `:17` `default_originator="nanobot"`.
- API: `openai_codex_provider.py:60` `DEFAULT_CODEX_URL = "https://chatgpt.com/backend-api/codex/responses"`. Models: `:61` `DEFAULT_OPENAI_CODEX_MODELS_URL = ".../backend-api/codex/models"`, fetched at `:809`, falls back to `registry.py` bundled list (`:537` onward).
- Own docs: `docs/troubleshooting.md:154` only tells users which model prefix to use. No terms text found.

### 4. ZeroClaw (`zeroclaw-labs/zeroclaw`, `cda933e6`)
A. Can also import `~/.codex/auth.json`.
- `crates/zeroclaw-providers/src/auth/openai_oauth.rs:18-22`: client id, authorize, token, `OPENAI_OAUTH_REDIRECT_URI = "http://localhost:1455/auth/callback"`. Listener binds `127.0.0.1:1455` (`:240`).
- `crates/zeroclaw-providers/src/openai_codex.rs:21` `DEFAULT_CODEX_RESPONSES_URL = "https://chatgpt.com/backend-api/codex/responses"`; sends `originator: pi` (`:1334`).
- Docs `docs/book/src/providers/openai-codex-subscription.md:103` tell the user to `curl ".../backend-api/codex/models?client_version=1.0.0"`. I found no `/models` call in the provider code.
- Terms text, same file `:272`: "OpenAI's terms prohibit sharing account credentials or making an account available to someone else, and a raw `auth.json` pull point is a high-value secret on its own." Context: copying `auth.json` to other hosts.

### 5. PicoClaw (`sipeed/picoclaw`, `bbf6893c`)
A. Repo last pushed 2026-11-05.
- `pkg/auth/oauth.go:44-48`: `Issuer: "https://auth.openai.com"`, `ClientID: "app_EMoamEEZ73f0CkXaXp7hrann"`, `Originator: "codex_cli_rs"`, `Port: 1455`. `:527` `cfg.Issuer + "/oauth/authorize?"`; `:187-188` redirect `http://localhost:%d/auth/callback`. Token: `cfg.Issuer + "/oauth/token"` (`:453`).
- `pkg/providers/oauth/codex_provider.go:34` `option.WithBaseURL("https://chatgpt.com/backend-api/codex")`.
- Models: no models endpoint found in `codex_provider.go`. Docs: no terms text found.

### 6. NanoClaw (`nanocoai/nanoclaw`, `2cb6926b`)
No sign-in of its own. The `/add-codex` skill runs OpenAI's `codex` binary.
- `.claude/skills/add-codex/payload/setup/providers/codex.ts:167` `const args = method === 'device' ? ['login', '--device-auth'] : ['login'];` run under a temporary `CODEX_HOME`, then the resulting `auth.json` goes into the vault (`:195`).
- `.../payload/src/provider-contracts/codex.ts:11` `token: 'https://auth.openai.com/oauth/token'` (a host allow-list entry, not a flow).
- README `:87`: "`/add-codex` for OpenAI's Codex (ChatGPT subscription or API key)".
- The client id used is whatever the pinned Codex CLI sends, so Method A, but NanoClaw does not contain the id.

### 7. opencode (`anomalyco/opencode`, formerly `sst/opencode`, `b03809b2`)
Both. Both live in `packages/core/src/plugin/provider/`.
- A (`openai.ts`): `:18` `const clientID = "app_EMoamEEZ73f0CkXaXp7hrann"`, `:20` `callbackPort = 1455`, `:25` `codexBaseURL = "https://chatgpt.com/backend-api/codex"`, `:410` `${issuer}/oauth/authorize?`. Methods `chatgpt-browser` and `chatgpt-headless` (`:26-27`). Model filter `:285` "ChatGPT-plan tokens only authorize codex-eligible models"; allow/deny list `:28-29`.
- B (`chatgpt.ts`): `:23` `registrationClientID = "dynamic_agent_client"`, `:24` `agentName = "OpenCode"`, `:26` token `${issuer}/api/accounts/oauth/token`, `:27` `resource = "https://api.openai.com/v1"`, `:154` redirect `http://127.0.0.1:${port}/auth/callback`, `:534` `${issuer}/api/accounts/authorize?`, `:300` `baseURL: resource`.
- Models (B): `:425` `data.models.filter((model) => model.visibility === "list" && model.supported_in_api)` after `GET ${baseURL}/models` (`:419`).
- Which one is the default: not verified. No terms text found in the repo's Markdown.

### 8. Cline (`cline/cline`, `eff43e19`)
A. Provider id `openai-codex`, label "Sign in with ChatGPT Subscription".
- `sdk/packages/core/src/auth/codex.ts:29-34`: authorize `https://auth.openai.com/oauth/authorize`, token `.../oauth/token`, `clientId: "app_EMoamEEZ73f0CkXaXp7hrann"`, `redirectUri: "http://localhost:1455/auth/callback"`. The VS Code app has its own copy (`apps/vscode/src/integrations/openai-codex/oauth.ts:20-24`).
- `sdk/packages/llms/src/providers/builtins.ts:1178` `defaults: { baseUrl: "https://chatgpt.com/backend-api/codex" }`; models `:454` `filterOpenAICodexModels(generatedModels("openai-native"))`.
- `builtins.ts:1170` description says "OpenAI ChatGPT subscription access uses an OAuth device code flow." I found no `deviceauth` URL in `codex.ts`, so the description and the code disagree. Not resolved.
- No `dynamic_agent_client` in the repo. No terms text found.

### 9. Roo Code (`RooCodeInc/Roo-Code`, `b867ec91`)
A. The repo is archived (GitHub API `archived: true`, last push 2026-05-15), so this is a snapshot, not a live app.
- `src/integrations/openai-codex/oauth.ts:18-23` authorize, token, `clientId: "app_EMoamEEZ73f0CkXaXp7hrann"`, `redirectUri: "http://localhost:1455/auth/callback"`; header comment `:15` "Codex-specific params: codex_cli_simplified_flow=true, originator=roo-code".
- `src/api/providers/openai-codex.ts:34` `CODEX_API_BASE_URL = "https://chatgpt.com/backend-api/codex"`. Models: `packages/types/src/providers/openai-codex.ts:26` bundled `openAiCodexModels`.
- No terms text found.

### 10. Goose (`aaif-goose/goose`, formerly `block/goose`, `d8962e32`)
A. Provider `chatgpt_codex`.
- `crates/goose/src/providers/chatgpt_codex.rs:38` `CLIENT_ID = "app_EMoamEEZ73f0CkXaXp7hrann"`, `:39` issuer, `:40` `CODEX_API_ENDPOINT = "https://chatgpt.com/backend-api/codex"`, `:44` port 1455, `:651` `{ISSUER}/oauth/authorize`, `:678` `{issuer}/oauth/token`, `:908` redirect `http://localhost:1455/auth/callback`, `:648` `originator=goose`.
- Models: `:58` bundled `CHATGPT_CODEX_KNOWN_MODELS`. No remote catalog call found.
- Docs `documentation/docs/getting-started/providers.md:33`: "Requires a ChatGPT Plus/Pro subscription." No terms text found.

### 11. pi (`earendil-works/pi`, formerly `badlogic/pi-mono`, `cead615b`)
Both. In pi's own words B supersedes A.
- A (`packages/ai/src/auth/oauth/openai-codex.ts`): `:22` `CLIENT_ID = "app_EMoamEEZ73f0CkXaXp7hrann"`, `:24` `/oauth/authorize`, `:26` `REDIRECT_URI = "http://localhost:1455/auth/callback"`; `:290` `originator: string = "pi"`. Base `packages/ai/src/providers/openai-codex.ts:11` `https://chatgpt.com/backend-api`.
- B (`packages/ai/src/auth/oauth/openai-chatgpt.ts`): `:16` `DYNAMIC_CLIENT_ID = "dynamic_agent_client"`, `:17` `AGENT_NAME_HINT = "Pi"`, `:19-20` authorize and token URLs, `:21` `RESOURCE = "https://api.openai.com/v1"`, `:25` `REDIRECT_URI = http://127.0.0.1:1455/auth/callback`, `:254` `ext_agent_host_id: hostId`. Header `:1-5`: "OpenAI Responses API token sharing through Sign in with ChatGPT."
- API base for B: `packages/ai/catalog/providers/openai.ts:20` `BASE_URL = "https://api.openai.com/v1"`. Models: bundled catalog; I found no `/v1/models` call.
- `packages/ai/README.md:1888`: "**OpenAI Codex**: Superseded by Sign in with ChatGPT on the OpenAI provider."
- Own docs: no terms text found.

### 12. OpenAI Codex CLI (`openai/codex`, `8011b92b`), reference for A
- `codex-rs/login/src/auth/manager.rs:1897` `pub const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";`
- `codex-rs/login/src/server.rs:78` `DEFAULT_ISSUER = "https://auth.openai.com"`, `:79` `DEFAULT_PORT = 1455`, `:197` `redirect_uri = format!("http://127.0.0.1:{actual_port}/auth/callback")`, `:593` `("codex_cli_simplified_flow", "true")`, `:609` `{issuer}/oauth/authorize`, `:722` `{issuer}/oauth/token`.
- `codex-rs/model-provider-info/src/lib.rs:87` `CHATGPT_CODEX_BASE_URL = "https://chatgpt.com/backend-api/codex"`. `codex-rs/login/src/auth/default_client.rs:42` `DEFAULT_ORIGINATOR = "codex_cli_rs"`.
- Models: `codex-rs/models-manager/src/manager.rs:409` "OpenAI-compatible model manager backed by bundled models, cache, and `/models`."
- Note: Codex CLI sends `127.0.0.1` as redirect host here; every third-party A app above sends `localhost`.

## What OpenAI says

All fetched 2026-11-17 as Markdown from the `.md` twin of each page.

**Method B is for open-source apps** ([token-sharing-open-source](https://developers.openai.com/siwc/token-sharing-open-source.md)):
- "ChatGPT plan usage is an optional capability within Sign in with ChatGPT. ... your open-source app can request permission to use the user's ChatGPT plan for eligible Responses API requests."
- "The OAuth flow for OSS clients registers a **user-defined agent** during Sign in with ChatGPT."

**Availability** ([quickstart](https://developers.openai.com/siwc/quickstart.md)):
- "Let users run AI workloads in your tools with their ChatGPT plan, without requiring them to provide an API key."
- "ChatGPT plan usage is available to all open-source partners and selected private clients."

**Which endpoint** ([models-and-inference](https://developers.openai.com/siwc/token-sharing-open-source/models-and-inference.md)):
- "Use the public Responses API endpoint above for this ChatGPT plan usage flow; do not point it at ChatGPT's `backend-api` endpoints."
- "Set `store` to `false` and `stream` to `true` on each HTTP inference request in this flow."
- Model list: `curl https://api.openai.com/v1/models ... select(.visibility == "list")`; "If you use Codex app-server, its `model/list` RPC may use a bundled or cached catalog."

**Registration and callback** ([sign-in](https://developers.openai.com/siwc/token-sharing-open-source/sign-in.md)):
- "`dynamic_agent_client` is the first-time registration entrypoint, not the client ID to save or use for token exchange."
- "Use an HTTP loopback callback on `127.0.0.1` ... Do not substitute with `localhost`." (OpenClaw's B flow names `localhost` in its redirect, see Table. Whether OpenAI accepts it: not verified.)
- `agent_name_hint`: "Your app's actual name, such as `OpenClaw`."

**Partner list** ([learn.chatgpt.com/docs/sign-in-with-chatgpt](https://learn.chatgpt.com/docs/sign-in-with-chatgpt.md)): "Open-source integrations:" lists OpenClaw, OpenCode, Pi, T3. "Apps with ChatGPT plan usage:" lists Hermes Agent, among others (Amp Code, Kilo Code, Warp, ...).

**Help Center** ([article 20001542](https://help.openai.com/en/articles/20001542-using-your-chatgpt-plan-in-other-apps-and-sites)):
- "Supported open-source tools remain available to all ChatGPT users."
- "Only sign in to apps and sites you trust."
- Users can set a weekly limit per app in Settings > Usage.

**Codex sign-in docs** ([learn.chatgpt.com/docs/auth](https://learn.chatgpt.com/docs/auth.md)). These cover OpenAI's own clients only:
- "The ChatGPT desktop app, Codex CLI, and IDE extension support both sign-in methods for local work."
- "Use API key authentication for programmatic Codex CLI workflows, such as CI/CD jobs."
- "Access tokens are intended for trusted scripts, schedulers, and private CI runners. For general OpenAI API calls, continue to use Platform API keys."

**General terms** ([Europe Terms of Use](https://openai.com/policies/eu-terms-of-use/), the page `openai.com/policies/terms-of-use/` redirected to it; updated January 16, 2026):
- "You may not share your account credentials or make your account available to anyone else."
- "Automatically or programmatically extracting data or Output" is listed under "What you cannot do".
- These are general terms. They do not mention Method A, the Codex client id or `backend-api`.

**Not found:** any OpenAI sentence that allows or forbids a third-party app to use `app_EMoamEEZ73f0CkXaXp7hrann` or `chatgpt.com/backend-api/codex`. The only related text is the "do not point it at ChatGPT's `backend-api` endpoints" line, which applies to Method B.

## Not verified

1. Model list for OpenClaw Method A: no `codex/models` call found in `extensions/openai` or `extensions/codex/src`; source of its Codex rows not traced (`codex-model-rows.ts` filters `visibility`).
2. Model list for PicoClaw and ZeroClaw: no models endpoint found in the provider code (searched `/models`). ZeroClaw docs describe a manual `curl`.
3. Hermes Agent and Method B: source has none, but OpenAI's partner page lists Hermes under "Apps with ChatGPT plan usage". Searched the repo for `dynamic_agent_client`, `ext_agent_host_id`, `chatgpt.tokens.use.direct`, `siwc`, `api/accounts/authorize`, and GitHub PRs/issues for "Sign in with ChatGPT". Nothing found. Hermes may ship B in a branch, a release after the surveyed commit, or via a Nous service I did not check.
4. Cline: description says "device code flow", code has loopback authorize. I did not trace which one runs.
5. opencode and pi: which of A or B is the default login was not verified (OpenClaw's default is A, from its docs).
6. T3 (`pingdotgg/t3code`), listed by OpenAI as an open-source integration, was not surveyed.
7. nanobot's client id lives in `oauth-cli-kit`, which has no GitHub URL in its PyPI metadata. Evidence is the 0.1.6 sdist; the version nanobot installs may be newer (`<1.0.0`).
8. Whether OpenAI accepts `localhost` (not `127.0.0.1`) for B redirects: OpenClaw does it, OpenAI's doc says not to. Not tested (I did not run or sign in to anything).
9. Whether OpenAI allows or forbids third-party use of Method A: no OpenAI text found. The EU Terms of Use sentences above were not tied to Method A by OpenAI. The US/ROW terms page (`row-terms-of-use`) was not read.
10. App-side terms or risk text: only ZeroClaw's credential-sharing note (and OpenClaw's "explicitly supported" claim) found. I searched Markdown docs of each repo for terms/risk/unofficial/violat wording; I did not read every doc.
11. NanoClaw: which Codex CLI version it pins and whether that CLI's client id changed: not checked.
12. Repo contents are from default branches on 2026-11-17. Apps may differ in tagged releases.

## Bate against pi (the app Bate follows first)

Compared 2026-11-17: Bate `appd/src/chatgpt.rs`, `agentcore/src/model.rs` against pi `cead615b`, `packages/ai/src/auth/oauth/openai-chatgpt.ts` and `packages/ai/src/api/openai-responses.ts`. pi offers both sign-ins; its README says the Codex one is "Superseded by Sign in with ChatGPT" (`packages/ai/README.md:1888`), so Method B is the one compared here. (pi labelled the Codex one "(legacy)" and later dropped the label: `packages/coding-agent/CHANGELOG.md:765`, `:104`.) oh-my-pi (omp, the second app Bate follows) has Method A only in the installed copy, so it has nothing to follow for this sign-in.

| Point | pi | Bate | Decision |
|---|---|---|---|
| Authorize URL, token URL, scope, `resource`, redirect `http://127.0.0.1:1455/auth/callback`, PKCE S256, `state`, `nonce`, `agent_name_hint`, `ext_agent_host_id` | `openai-chatgpt.ts:16-27,252-264` | `chatgpt.rs:14,125-160` | Same. |
| Granted scope must hold `chatgpt.tokens.use.direct` | `:170` | `chatgpt.rs` `finish` | Same. |
| Requests: `store:false`, `stream:true`, no `temperature`, `max_output_tokens`, `prompt_cache_retention` | `openai-responses.ts:363-381` | `model.rs` `responses_body` | Same (Bate sends none of them). |
| Plan usage limit: point at `chatgpt.com/settings/usage`, do not retry | `openai-responses.ts:260-261`, `utils/retry.ts:27` | `model.rs` | Same. |
| Workspace usage limit (`subscription_sharing_workplace_individual_usage_limit_exceeded`): tell the user to ask the admin | `openai-responses.ts:262-263` | was the raw error | Adopted from pi. |
| Renew the access token before it ends | 3 minutes (`:29`) | was 1 minute | Adopted from pi. |
| Registration | a new one on every login (`:15`) | keeps the issued client id | Keep Bate: OpenAI's doc says `dynamic_agent_client` is "the first-time registration entrypoint". |
| Port 1455 taken | fails with a message (`:246`) | ends its own earlier attempt, else another port | Keep Bate: the doc lets the port differ. |
| ID token | must be present (`:203`) | nonce must match | Keep Bate (stricter). |
| Sign-out | no revoke call in `openai-chatgpt.ts` | revokes the refresh token, then forgets | Keep Bate. |
| Model list | bundled catalog; default `gpt-6.1-sol` (`CHANGELOG.md:216`) | `GET /v1/models`, `visibility == "list"`, first one is the default | Keep Bate: this is what the doc says, and what OpenClaw and opencode do. |
| Paste the redirect URL by hand | yes (`:64-80`) | no | Not needed: Bate's browser is on the same Mac. |
| `prompt_cache_key` (the session id) | sent (`openai-responses.ts:369`) | not sent | Open. |
| Retry `subscription_sharing_usage_unavailable` and `subscription_sharing_user_unavailable` | yes (`retry.ts:137-138`) | Bate retries no model error | Open. |
