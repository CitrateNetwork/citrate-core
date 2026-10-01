// HUP-S3.4 — the learn proposal card: what Hermes wants to keep, the verifier evidence behind it,
// the conflicts it would cause, and the member's controls (acknowledge, accept, reject, and for a
// saved skill, publish). Pure view of `proposalCardModel`; decisions go to the callbacks.
import { useState } from "react";
import { proposalCardModel, type LearnProposal, type PublishAvailability } from "../agent/learn";

const label = { fontSize: 9.5, letterSpacing: ".1em", textTransform: "uppercase" as const, color: "var(--tx-3)" };

export interface LearnProposalCardProps {
  proposal: LearnProposal;
  publish: PublishAvailability | null;
  acked: ReadonlySet<string>;
  busy?: boolean;
  onAck(conflictId: string, on: boolean): void;
  onAccept(): void;
  onReject(reason: string): void;
  onPublish?(): void;
}

export function LearnProposalCard({ proposal, publish, acked, busy, onAck, onAccept, onReject, onPublish }: LearnProposalCardProps) {
  const m = proposalCardModel(proposal, acked, publish);
  const [rejecting, setRejecting] = useState(false);
  const [reason, setReason] = useState("");
  const [showBody, setShowBody] = useState(false);
  return (
    <div data-testid="learn-card" data-kind={m.kindLabel.toLowerCase()} style={{ border: "1px solid var(--line-2)", borderRadius: "var(--r-1)", overflow: "hidden" }}>
      <div style={{ display: "flex", alignItems: "center", gap: 10, padding: "8px 12px", background: "var(--srf-1)" }}>
        <span className="mono" style={label}>
          {m.kindLabel}
        </span>
        <span data-testid="learn-title" style={{ fontSize: 13, fontWeight: 500, color: "var(--tx-1)", minWidth: 0, overflowWrap: "anywhere" }}>
          {m.title}
        </span>
        <span data-testid="learn-state" className="mono" style={{ marginLeft: "auto", fontSize: 10, color: m.awaiting ? "var(--warn)" : "var(--tx-3)", flexShrink: 0 }}>
          {m.stateLabel}
        </span>
      </div>
      <div style={{ padding: "8px 12px", display: "flex", flexDirection: "column", gap: 8 }}>
        {m.subtitle && <span style={{ fontSize: 12.5, color: "var(--tx-2)", overflowWrap: "anywhere" }}>{m.subtitle}</span>}
        {m.body !== null && (
          <div>
            <button className="btn btn-ghost btn-sm" onClick={() => setShowBody((v) => !v)}>
              {showBody ? "Hide the skill" : "Read the skill"}
            </button>
            {showBody && (
              <pre data-testid="learn-body" className="mono" style={{ margin: "6px 0 0", fontSize: 11, whiteSpace: "pre-wrap", overflowWrap: "anywhere", color: "var(--tx-1)", background: "var(--srf-1)", padding: 8, borderRadius: 4 }}>
                {m.body}
              </pre>
            )}
          </div>
        )}

        {/* evidence: only verifiers say done */}
        <div data-testid="learn-evidence" style={{ display: "flex", flexDirection: "column", gap: 2, borderTop: "1px solid var(--line-1)", paddingTop: 6 }}>
          <span className="mono" style={label}>
            Evidence: workflow {m.evidence.workflow}
          </span>
          {m.evidence.verdicts.map((v, i) => (
            <span key={i} data-testid="learn-verdict" data-pass={v.passed ? "true" : "false"} style={{ fontSize: 12, color: "var(--tx-1)" }}>
              <span className="mono" style={{ color: v.passed ? "var(--ok)" : "var(--danger)", marginRight: 6 }}>
                {v.passed ? "pass" : "fail"}
              </span>
              {v.label}
            </span>
          ))}
          <span className="mono" style={{ fontSize: 10.5, color: "var(--tx-3)", overflowWrap: "anywhere" }}>
            {m.evidence.attempts} · {m.evidence.trajectory}
            {m.evidence.model ? ` · model ${m.evidence.model}` : ""}
          </span>
        </div>

        {/* conflicts: surfaced, never merged */}
        {m.conflicts.length > 0 && (
          <div style={{ display: "flex", flexDirection: "column", gap: 4, borderTop: "1px solid var(--line-1)", paddingTop: 6 }}>
            <span className="mono" style={label}>
              Conflicts
            </span>
            {m.conflicts.map((c) => (
              <label key={c.id} data-testid="learn-conflict" data-blocking={c.blocking ? "true" : "false"} style={{ display: "flex", gap: 8, alignItems: "flex-start", fontSize: 12, color: c.blocking ? "var(--danger)" : "var(--tx-1)" }}>
                {!c.blocking && m.awaiting && (
                  <input type="checkbox" aria-label={`Acknowledge: ${c.label}`} checked={c.acknowledged} onChange={(e) => onAck(c.id, e.currentTarget.checked)} />
                )}
                <span>
                  <strong style={{ fontWeight: 500 }}>{c.label}.</strong> {c.detail}
                  {!c.blocking && " Accepting keeps both; both are marked unresolved and linked as contradicting, and nothing is merged."}
                </span>
              </label>
            ))}
          </div>
        )}

        {m.awaiting && (
          <div style={{ display: "flex", flexDirection: "column", gap: 6, borderTop: "1px solid var(--line-1)", paddingTop: 8 }}>
            {m.acceptBlocked && (
              <span data-testid="learn-accept-blocked" style={{ fontSize: 11.5, color: "var(--tx-3)" }}>
                {m.acceptBlocked}
              </span>
            )}
            {rejecting ? (
              <div style={{ display: "flex", gap: 6, alignItems: "center" }}>
                <input aria-label="Why (optional)" placeholder="Why (optional)" value={reason} onChange={(e) => setReason(e.currentTarget.value)} style={{ flex: 1, minWidth: 0, fontSize: 12 }} />
                <button className="btn btn-secondary btn-sm" disabled={busy} onClick={() => onReject(reason)}>
                  Reject
                </button>
                <button className="btn btn-ghost btn-sm" onClick={() => setRejecting(false)}>
                  Back
                </button>
              </div>
            ) : (
              <div style={{ display: "flex", gap: 6 }}>
                <button data-testid="learn-accept" className="btn btn-primary btn-sm" disabled={!m.canAccept || busy} onClick={onAccept}>
                  Accept
                </button>
                <button data-testid="learn-reject" className="btn btn-ghost btn-sm" disabled={busy} onClick={() => setRejecting(true)}>
                  Reject
                </button>
              </div>
            )}
          </div>
        )}

        {m.publish && (
          <div data-testid="learn-publish" style={{ display: "flex", flexDirection: "column", gap: 4, borderTop: "1px solid var(--line-1)", paddingTop: 8 }}>
            <button className="btn btn-secondary btn-sm" disabled={!m.publish.enabled || busy || !onPublish} onClick={onPublish} style={{ alignSelf: "flex-start" }}>
              Publish to the SkillRegistry
            </button>
            <span data-testid="learn-publish-note" style={{ fontSize: 11.5, color: "var(--tx-3)" }}>
              {m.publish.note}
            </span>
          </div>
        )}
      </div>
    </div>
  );
}
