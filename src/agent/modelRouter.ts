// Hermes ModelRouter — the pure selection core (P0 / WP0.1).
//
// The router is the SINGLE backend selector every agent surface reads. It enumerates
// model choices from THREE sources — local (downloaded + verified GGUFs), the on-chain
// registry, and the always-ready gateway/Gemma — and resolves the one the send path uses.
// Pure + framework-free so the guarantees are unit-testable without a chain, a bridge, or
// a running model; they mirror the TLA+ model in src-tauri/formal/ModelRouter.tla:
//   INV-Router-1  exactly one active selection (activeId is a single value).
//   INV-Router-2  the send path never serves a NOT-ready model — resolveActive returns the
//                 active choice iff ready, else the always-ready gateway terminal.
//   INV-Router-3  no phantom — only an enumerated choice can be selected, and resolveActive
//                 always returns an enumerated choice (never a fabricated id, Rule 1).
// (WP0.2 wires the three real sources to enumerateChoices; this module owns the logic.)

export type ModelSource = "local" | "registry" | "gateway";

export interface ModelChoice {
  /** Stable id used for selection + the send path. */
  id: string;
  /** Human label for the picker. */
  label: string;
  /** Which source surfaced this choice. */
  source: ModelSource;
  /** Ready to serve NOW? Local = downloaded+verified; registry = pulled+verified; gateway = configured. */
  ready: boolean;
}

/** The always-available terminal backend. The app guarantees a ready gateway/Gemma (or, when
 *  the gateway is unconfigured, the on-device demo terminal — wired in WP0.2); the router
 *  treats it as the ready fallback so the send path is never left with nothing to serve. */
export const GATEWAY_ID = "gateway" as const;
export const GATEWAY_CHOICE: ModelChoice = { id: GATEWAY_ID, label: "Citrate gateway", source: "gateway", ready: true };

export interface EnumerateInput {
  /** Locally downloaded + verified models (ready). */
  local: { id: string; label: string }[];
  /** On-chain-registry models not yet local (NOT ready — must pull+verify first). */
  registry: { id: string; label: string }[];
  /** Is the gateway configured (a sealed key)? Defaults true (the out-of-box terminal). */
  gatewayReady?: boolean;
}

/** Merge the three sources into the router's choice list. A registry model that is also
 *  present locally is surfaced ONCE, as the (ready) local choice — no duplicate id. */
export function enumerateChoices(input: EnumerateInput): ModelChoice[] {
  const localIds = new Set(input.local.map((m) => m.id));
  const local: ModelChoice[] = input.local.map((m) => ({ id: m.id, label: m.label, source: "local", ready: true }));
  const registry: ModelChoice[] = input.registry
    .filter((m) => !localIds.has(m.id))
    .map((m) => ({ id: m.id, label: m.label, source: "registry", ready: false }));
  const gateway: ModelChoice = { ...GATEWAY_CHOICE, ready: input.gatewayReady ?? true };
  return [...local, ...registry, gateway];
}

/** INV-Router-3 gate: only an enumerated choice may be selected (no phantom id). */
export function canSelect(id: string, choices: ModelChoice[]): boolean {
  return choices.some((c) => c.id === id);
}

/** The gateway terminal within a choice list (falls back to the constant if absent). */
export function gatewayChoice(choices: ModelChoice[]): ModelChoice {
  return choices.find((c) => c.source === "gateway") ?? GATEWAY_CHOICE;
}

/** INV-Router-2: the backend the send path actually serves — the active choice iff it is
 *  READY, else the always-ready gateway terminal. Selecting a not-ready model is allowed
 *  (it triggers the real download/pull); until it is ready, the send path uses the gateway. */
export function resolveActive(activeId: string | null, choices: ModelChoice[]): ModelChoice {
  const active = activeId == null ? undefined : choices.find((c) => c.id === activeId);
  return active && active.ready ? active : gatewayChoice(choices);
}
