// =====================================================================
// citrate-core: honest failure copy for a Hermes reply (FL design board 01, "Honest failure")
//
// A reply that did not finish gets one sentence naming what failed and one naming the fix, never a
// bare spinner or a raw error alone. The cases come from what the send path really reports
// (store.sendChat, sidecarProvider, harness): the member stopped it; the model did not answer;
// Hermes stopped answering mid-run; the local model is not running; anything else keeps the
// reason as it was reported. A refused tool call is not a failed reply: its step in the run
// timeline says "refused" with core's own reason (runTimeline.ts).
// =====================================================================
import type { SidecarDownReason } from "./sidecarLiveness";

export type FailureKind = "stopped" | "model_down" | "hermes_lost" | "local_not_running" | "not_sent" | "other";

/** What the reply's fix button does (absent = the sentence is the fix). */
export type FailureAction = "retry" | "pick_model" | "start_hermes" | "open_controls";

export interface FailureCopy {
  kind: FailureKind;
  sentence: string;
  fix: string;
  actions: FailureAction[];
}

const MODEL_DOWN = /(connection refused|econnrefused|failed to connect|could not connect|error sending request|model (server|request)|llama-server|upstream|\b50[234]\b|timed out|timeout)/i;

/** The copy for a reply that ended with `error` (the reason the send path recorded). */
export function failureCopy(error: string): FailureCopy {
  const e = (error || "").trim();
  if (/^stopped by you$/i.test(e)) {
    return { kind: "stopped", sentence: "You stopped this reply. Nothing after the stop was run.", fix: "Retry to run it again from the start.", actions: ["retry"] };
  }
  if (/local model is not running/i.test(e)) {
    return {
      kind: "local_not_running",
      sentence: "The local model is not running, so nothing was sent.",
      fix: "Start the local model in Hermes › Controls › Models, or pick another model, then retry.",
      actions: ["pick_model", "retry"],
    };
  }
  if (/^not sent\b/i.test(e)) {
    return { kind: "not_sent", sentence: "This message was not sent.", fix: "Retry when you are ready to choose where it goes.", actions: ["retry"] };
  }
  if (/session stopped responding|session was stopped/i.test(e)) {
    return {
      kind: "hermes_lost",
      sentence: "Hermes stopped answering in the middle of this reply.",
      fix: "Retry. If it happens again, restart Hermes in Hermes › Controls.",
      actions: ["retry", "open_controls"],
    };
  }
  if (MODEL_DOWN.test(e)) {
    return {
      kind: "model_down",
      sentence: `The model did not answer (${e}).`,
      fix: "Check that the model is running, or pick another model, then retry.",
      actions: ["pick_model", "retry"],
    };
  }
  return { kind: "other", sentence: `Hermes did not finish this reply: ${e || "no reason was reported"}.`, fix: "Retry, or ask in a different way.", actions: ["retry"] };
}

/** The banner shown when chat is on the no-tools fallback, with its fix. */
export function hermesDownCopy(reason: SidecarDownReason): { title: string; sentence: string; action: "start_hermes" | "open_settings" | "open_controls" } {
  if (reason.why === "loop_off") {
    return {
      title: "Hermes's agent loop is off",
      sentence: "Replies come from the local model with no tools: Hermes cannot read your node, wallet, memory or files. Your chats are sealed and safe.",
      action: "open_settings",
    };
  }
  if (reason.why === "unreadable") {
    return {
      title: "Hermes's status could not be read",
      sentence: `The status read failed (${reason.error}), so replies come from the local model with no tools. Your chats are sealed and safe.`,
      action: "open_controls",
    };
  }
  return {
    title: "Hermes is not running",
    sentence: "Replies come from the local model with no tools until Hermes starts. Your chats are sealed and safe. Nothing was sent to Hermes.",
    action: "start_hermes",
  };
}
