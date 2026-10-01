// HUP-S6.4 — the D-4 deploy gate verdict card: READY / NOT READY for one exact bytecode hash,
// with one line per gate item (forge tests, Slither, Aderyn, Medusa campaign, fork dry run)
// and its evidence. Pure view of `gateCardModel`; it never decides anything (core does).
import { gateCardModel, type DeployGateRecord } from "../agent/deployGate";

const label = { fontSize: 9.5, letterSpacing: ".1em", textTransform: "uppercase" as const, color: "var(--tx-3)" };

export function DeployGateCard({ record, initcodeHash }: { record: DeployGateRecord | null | undefined; initcodeHash: string }) {
  const m = gateCardModel(record, initcodeHash);
  const tone = m.ready
    ? { fg: "var(--ok)", bg: "var(--ok-bg)", bd: "var(--ok)" }
    : { fg: "var(--danger)", bg: "var(--danger-bg)", bd: "var(--danger)" };
  return (
    <div data-testid="deploy-gate-card" role="group" aria-label={"Deploy gate: " + m.verdict} style={{ border: "1px solid " + tone.bd, borderRadius: "var(--r-1)", overflow: "hidden" }}>
      <div style={{ display: "flex", alignItems: "center", gap: 10, padding: "8px 12px", background: tone.bg }}>
        <span className="mono" style={{ ...label, color: tone.fg }}>
          Deploy gate
        </span>
        <span data-testid="deploy-gate-verdict" className="mono" style={{ marginLeft: "auto", fontSize: 11, fontWeight: 600, letterSpacing: ".08em", color: tone.fg }}>
          {m.verdict}
        </span>
      </div>
      <div style={{ padding: "8px 12px", display: "flex", flexDirection: "column", gap: 6, background: "var(--srf-1)" }}>
        <div style={{ display: "flex", flexDirection: "column", gap: 2 }}>
          <span className="mono" style={label}>
            Bytecode hash (keccak256 of bytecode and constructor args)
          </span>
          <span data-testid="deploy-gate-hash" className="mono" style={{ fontSize: 11, color: "var(--tx-1)", wordBreak: "break-all" }}>
            {m.initcodeHash}
          </span>
          {m.compiler && (
            <span className="mono" style={{ fontSize: 10.5, color: "var(--tx-3)" }}>
              {m.compiler}
            </span>
          )}
        </div>
        {m.reasons.length > 0 && (
          <ul style={{ margin: 0, paddingLeft: 18, display: "flex", flexDirection: "column", gap: 2 }}>
            {m.reasons.map((r, i) => (
              <li key={i} data-testid="deploy-gate-reason" style={{ fontSize: 12, lineHeight: 1.45, color: "var(--danger)" }}>
                {r}
              </li>
            ))}
          </ul>
        )}
        {m.items.length > 0 && (
          <div style={{ display: "flex", flexDirection: "column", borderTop: "1px solid var(--line-1)", paddingTop: 6, gap: 4 }}>
            {m.items.map((it) => (
              <div key={it.id} data-testid="deploy-gate-item" data-pass={it.pass ? "true" : "false"} style={{ display: "flex", flexDirection: "column", gap: 1 }}>
                <span style={{ fontSize: 12, color: "var(--tx-1)" }}>
                  <span className="mono" style={{ color: it.pass ? "var(--ok)" : "var(--danger)", marginRight: 6 }}>
                    {it.pass ? "pass" : "fail"}
                  </span>
                  {it.label}: {it.reason}
                </span>
                <span className="mono" style={{ fontSize: 10.5, color: "var(--tx-3)", wordBreak: "break-all" }}>
                  {it.evidence}
                </span>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
