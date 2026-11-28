// citrate-core: bridge domain `chat`, moved verbatim from src/bridge/domains.ts by L0-WP05
// (planset v060). src/bridge/domains.ts re-exports it, so imports from "../bridge/domains" keep
// working. The owning lane is the row for this path in .agentile/v60-ownership.map.

/** CORE-AI1 (@rule8) — non-secret status of a configured AI provider. Carries the
 * id, https baseURL, model, and a `configured` flag — NEVER the API key or the
 * Authorization header (invariant 1). Mirrors the Rust `ProviderStatus`. */
/** K2: a core-minted, one-time consent to send one held message to a remote provider. */
export interface ChatConsent {
  consentId: string;
  providerId: string;
  expiresAtMs: number;
}

export interface AiProviderStatus {
  id: string;
  baseURL: string;
  model: string;
  configured: boolean;
  isDefault: boolean;
}

export interface ChatDomain {
  /**
   * CORE-AI1 (@rule8) — seal a provider's `{baseURL, model, apiKey}` in the OS
   * keyring, binding the key to its https baseURL. Returns nothing — the key is
   * NEVER returned (invariant 1). The Tauri impl invokes `ai_set_provider`; the
   * sim impl is honestly Unavailable (the web preview has no OS keyring).
   */
  setProvider(providerId: string, baseURL: string, model: string, apiKey: string): Promise<void>;
  /**
   * CORE-AI1 (@rule8) — non-secret status for the preset provider ids: `{id,
   * baseURL, model, configured, isDefault}`, NEVER the key. Drives the Settings
   * rows + the store's real-vs-demo provider selection.
   */
  providerStatus(): Promise<AiProviderStatus[]>;
  /** CORE-AI1 — delete a provider's sealed config (and clear the default if it
   * pointed here). */
  clearProvider(providerId: string): Promise<void>;
  /**
   * CORE-AI1 (@rule8) — REAL inference. The webview picks WHICH configured
   * provider id; Rust reads the STORED baseURL for that id and POSTs the OpenAI
   * `/v1/chat/completions` body with the sealed Bearer, returning the model
   * completion. The webview can NEVER supply the URL (exfil-binding, invariant 3).
   * The Tauri impl invokes `ai_chat`; the sim impl is honestly Unavailable.
   */
  infer(providerId: string, messagesJson: string, contextJson: string, consentId: string): Promise<string>;
  /**
   * K2 — the app is asking the member whether to send a held message to the gateway. Core mints
   * the one-time consent that `infer` / `inferTools` need for that message (`chat_consent_prepare`);
   * a remote send without it is refused in core. The sim impl is honestly Unavailable.
   */
  consentPrepare(providerId: string, message: string): Promise<ChatConsent>;
  /**
   * K2 — the question for a held message closed without "Send this message to the gateway this
   * time" (cancelled, stopped, or the local model answers instead). Core drops the consent it
   * minted for it (`chat_consent_drop`), so it cannot be used later. An id that is no longer open
   * is not an error. The sim impl has nothing to drop.
   */
  consentDrop(consentId: string): Promise<void>;
  /**
   * W3.3 — one AGENTIC turn: like `infer`, but the request carries a `tools` spec
   * and the result is the assistant MESSAGE JSON (content and/or `tool_calls`) so
   * the frontend runs the tool loop. Rust reads the STORED baseURL + sealed key
   * (invariants 1 + 3). The Tauri impl invokes `ai_chat_tools`; the sim impl is
   * honestly Unavailable.
   */
  inferTools(
    providerId: string,
    messagesJson: string,
    toolsJson: string,
    contextJson: string,
    consentId: string,
  ): Promise<string>;
  /**
   * BC-3.2 — REAL LOCAL inference against the bundled `llama-server` (llama.cpp)
   * on the loopback endpoint, with NO api key. The endpoint is derived IN RUST
   * from the serve manager's loopback port — the webview supplies ONLY the
   * messages + context, NEVER a URL or host (exfil-binding). The Tauri impl
   * invokes `ai_chat_local`; the sim impl is honestly Unavailable (the web
   * preview has no llama-server). Local routing only happens when
   * `inferenceState()` reports the server is healthy — otherwise the store falls
   * through to gateway/demo (never a fabricated local reply, Rule 1).
   */
  inferLocal(messagesJson: string, contextJson: string): Promise<string>;
  /**
   * The AGENTIC local path: like `inferLocal` but attaches the `tools` spec and returns the
   * assistant MESSAGE JSON (content and/or tool_calls) so the Hermes tool loop runs on the bundled
   * local model, not just the gateway. Tauri invokes `ai_chat_local_tools`; sim is Unavailable.
   */
  inferLocalTools(messagesJson: string, toolsJson: string, contextJson: string): Promise<string>;
  /**
   * BC-3.2 — the HONEST inference-routing state (kebab): `ready` (local model +
   * healthy server → chat runs LOCALLY), `local-fallback`/`gateway-only` (route
   * to the gateway), `downloading`, `no-model`, or `demo`. Computed IN RUST from
   * the real model status + serve health + `gatewayConfigured` (a cgk_ key is
   * sealed). Drives the store's local-vs-gateway-vs-demo selection so the route
   * is honest — the frontend never claims a local model that isn't serving. The
   * Tauri impl invokes `model_inference_state`; the sim impl returns `demo`.
   */
  inferenceState(gatewayConfigured: boolean): Promise<string>;
  /** K2: the member's chat route, stored in core (chat_route.rs). An unreadable setting reads as
   *  `local_only` with `loadError` set (a read error never widens what may leave the device).
   *  Tauri invokes `chat_route_get`; sim reports the default. */
  route(): Promise<ChatRouteStatus>;
  /** K2: store the chat route. Tauri invokes `chat_route_set`; sim is Unavailable. */
  setRoute(route: ChatRoute): Promise<ChatRouteStatus>;
}

/** K2: where chat may go. `ask_first` (default): ask before each message goes to the gateway.
 *  `local_only`: never to the gateway. Mirrors Rust `chat_route::ChatRoute` (snake_case). */
export type ChatRoute = "local_only" | "ask_first";
export interface ChatRouteStatus {
  route: ChatRoute;
  loadError: string | null;
}
/** K2: what the held-message card may offer: start the local model, and/or send this message to
 *  the gateway this time. */
export interface RouteAskOptions {
  restart: boolean;
  gateway: boolean;
  /** D-11: the remote the held message offers, when it is the preferred cloud model (absent = the
   *  gateway preset). */
  remoteLabel?: string;
}
