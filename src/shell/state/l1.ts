// =====================================================================
// citrate-core: an AppState part (L0-WP06)
//
// Lane L1 (Agent runtime: the Hermes turn engine, its provider and model route, personas, skills,
// the brief).
//
// L0-WP06 (lanes/L0_INTEGRATION_SEAMS.md 3.4). Owned by L1 (.agentile/v60-ownership.map).
// These fields moved here from state.ts verbatim, each with its freshState default and its
// PERSIST_SCOPE entry. state.ts composes AppState, freshState and PERSIST_SCOPE from every part;
// a field lives in exactly one part (src/shell/seams/statePartition.test.ts).
//
// New 0.6.0 state does not go here: it goes into a slice registered with registerSlice
// (src/shell/slices/registry.ts). This part changes when one of its legacy fields moves or retires.
// The persona seeding in freshState (the P2 to P4 sim values) stays in state.ts.
// =====================================================================

import type { QueuedChat, AcceptedBrief } from "../state";
import type { ChatRoute, HermesPersona, LocalSkill, RouteAskOptions } from "../../bridge/domains";
import type { SidecarDownReason } from "../../agent/sidecarLiveness";
import type { UserSkill } from "../../agent/userSkills";
import type { PersistScope } from "../netScope";

export interface L1State {
  chatStatus: "ready" | "thinking" | "streaming" | "tool";
  /** SCL-S7.5a (D-16 extended) — a chat message held because the local model is configured but
   *  its server is not running. `waiting`: the app's own startup start is pending (bounded wait);
   *  `ask`: the member chooses, for this message only, to restart the local model or send it to
   *  the gateway; `restarting`: the member chose restart. Null when nothing is held. */
  chatRouteHold: { msgId: string; phase: "waiting" | "ask" | "restarting"; options?: RouteAskOptions } | null;
  /** Board 01: follow-ups typed while a run was live, in send order. Not persisted. */
  chatQueue: QueuedChat[];
  /** #690: the context kinds (hermesContext.ts row ids) the member turned off for this chat.
   *  Core refuses their tools here. Kept with the chat on screen; a new chat starts with none. */
  chatContextOff: string[];
  /** Board 01: why chat is on the no-tools fallback (sidecarLiveness.ts), null when Hermes answers.
   *  Set each time the provider is rebuilt. Not persisted. */
  chatHermesDown: SidecarDownReason | null;
  /** K2: the member's chat route, read from core (chat_route.rs) and never stored in the browser.
   *  `ask_first` (default): ask before each message goes to the gateway. `local_only`: never. */
  chatRoute: ChatRoute;
  chatBackend: "gateway" | "local";
  /** Honest live label for the CURRENT chat provider (set by rebuildProvider from provider.label).
   *  Reflects what actually answers — local llama-server, the gateway, or the built-in demo. */
  chatProviderLabel: string;
  /** The current provider's kind — drives the status-dot colour (green local/gateway, amber demo). */
  chatProviderKind: "local" | "real" | "demo";
  /** Hermes ModelRouter (P0/WP0.4): the member's chosen model id (a local model, a registry
   *  id, or the gateway). null ⇒ the always-ready gateway default. Persisted. */
  activeModelId: string | null;
  /** Hermes P5: prompt-skills kept in app state by earlier builds. HUP-S3.2 moves them to SKILL.md
   *  files on this device (`localSkills`) on launch; one that cannot move stays here with a notice. */
  userSkills: UserSkill[];
  /** HUP-S3.2: the member's saved skills, read from the SKILL.md files the sidecar's one loader also
   *  reads (skills_local.rs). Not persisted here; the files are the record. */
  localSkills: LocalSkill[];
  /** HUP-S1.1c — run Hermes's loop in the sidecar. On by default (owner decision 2026-10-01); the
   * app falls back to its own loop whenever the sidecar is not running. */
  hermesSidecarLoop: boolean;
  /** One-time migration marker: the 2026-10-01 default (loop on) has been applied to this install.
   * After that, the member's own choice is kept. */
  hermesSidecarLoopDefaultApplied: boolean;
  /** D-11 (ADR P-5): the member's preferred cloud model, a mirror of core's setting
   *  (`providers_preferred_get`, `<app data>/providers/preferred.json`); null = the local model.
   *  Never persisted here: core is the source. */
  hermesCloudModel: { providerId: string; model: string } | null;
  /** HUP-S1.4 — the last accepted interview brief (persisted). */
  hermesBrief: AcceptedBrief | null;
  /** HUP-S3.3 + S3.7 — the chosen Hermes persona (its sidecar view, fragment included), or null for
   *  the default voice (the default: nothing about the prompt changes). Persisted. */
  hermesPersona: HermesPersona | null;
  /** HUP-S3.3 (US-3.3 AC3) — member-defined personas, each checked by the sidecar. Persisted. */
  customPersonas: HermesPersona[];
}

export function freshL1(_pid: string): L1State {
  return {
    chatStatus: "ready",
    chatRouteHold: null,
    chatQueue: [],
    chatHermesDown: null,
    chatContextOff: [],
    chatRoute: "ask_first",
    chatBackend: "gateway",
    chatProviderLabel: "starting…",
    chatProviderKind: "demo",
    activeModelId: null,
    userSkills: [],
    localSkills: [],
    hermesSidecarLoop: true,
    hermesSidecarLoopDefaultApplied: true,
    hermesCloudModel: null,
    hermesBrief: null,
    hermesPersona: null,
    customPersonas: [],
  };
}

const D = "device" as const;
export const L1_PERSIST_SCOPE = {
  activeModelId: D, chatBackend: D, userSkills: D, hermesSidecarLoop: D, hermesSidecarLoopDefaultApplied: D,
  hermesBrief: D, hermesPersona: D, customPersonas: D,
} satisfies Partial<Record<keyof L1State, PersistScope>>;
