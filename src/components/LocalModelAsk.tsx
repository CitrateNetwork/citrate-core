// =====================================================================
// citrate-core: ask before sending to the gateway (SCL-S7.5a, D-16 extended, US-6.2 AC5)
//
// Shown under a chat message that is held because the local model is set up but its server is not
// running. The member chooses, for this one message, to restart the local model or to send it to
// the gateway. Nothing is preselected and nothing is remembered: the next message asks again.
// While the app's own startup start of the local server is pending, it only says it is waiting.
// =====================================================================
import type { LocalModelChoice } from "../shell/store";

export const LOCAL_ASK_COPY = {
  askTitle: "Your local model is not running.",
  askBody: "This message has not been sent. Choose where it goes; this choice covers only this message.",
  restart: "Restart the local model",
  gateway: "Send this message to the gateway this time",
  cancel: "Cancel",
  waiting: "Waiting for your local model to start. This message has not been sent anywhere yet.",
  restarting: "Restarting your local model. This message will be answered on this device once it is ready.",
} as const;

export function LocalModelAsk({
  phase,
  onChoose,
  compact = false,
}: {
  phase: "waiting" | "ask" | "restarting";
  onChoose: (choice: LocalModelChoice) => void;
  /** Smaller type for the Journal's side chat. */
  compact?: boolean;
}) {
  const fs = compact ? 11.5 : 12.5;
  return (
    <div
      data-testid="local-model-ask"
      role={phase === "ask" ? "group" : "status"}
      aria-label={phase === "ask" ? LOCAL_ASK_COPY.askTitle : undefined}
      style={{ display: "flex", flexDirection: "column", gap: 8, border: "1px solid var(--line-2)", borderRadius: 8, padding: "10px 12px", background: "var(--srf-1)", fontSize: fs, lineHeight: 1.5, color: "var(--tx-1)" }}
    >
      {phase === "waiting" && <span>{LOCAL_ASK_COPY.waiting}</span>}
      {phase === "restarting" && <span>{LOCAL_ASK_COPY.restarting}</span>}
      {phase === "ask" && (
        <>
          <span>
            <strong style={{ fontWeight: 560 }}>{LOCAL_ASK_COPY.askTitle}</strong> {LOCAL_ASK_COPY.askBody}
          </span>
          <span style={{ display: "flex", gap: 8, flexWrap: "wrap", alignItems: "center" }}>
            <button className="btn btn-sm" data-testid="local-model-ask-restart" onClick={() => onChoose("restart")}>
              {LOCAL_ASK_COPY.restart}
            </button>
            <button className="btn btn-sm btn-secondary" data-testid="local-model-ask-gateway" onClick={() => onChoose("gateway")}>
              {LOCAL_ASK_COPY.gateway}
            </button>
            <button className="btn btn-sm btn-ghost" data-testid="local-model-ask-cancel" onClick={() => onChoose("cancel")}>
              {LOCAL_ASK_COPY.cancel}
            </button>
          </span>
        </>
      )}
    </div>
  );
}
