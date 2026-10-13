// =====================================================================
// citrate-core — the contract toolchain card (HUP-S6.3; v0.5.0-rc.5 FIX 5)
//
// The member's switch for Hermes's forge, slither, aderyn and medusa tools, which programs core
// found (and solc 0.8.36), the install command for each missing one on this OS, a Re-check that
// re-runs core's probe, and Restart Hermes (changes apply when Hermes next starts). Step 1 of
// the Agent surface's "Build a contract" panel (Agent > Contracts > Toolchain). The app does not
// install these tools in 0.5.0; everything shown is core's real probe result.
// =====================================================================
import { useEffect, useState, type CSSProperties } from "react";
import type { ToolchainStatus } from "../agent/contractForge";
import { formatSteps, hostOs, toolchainAdvice, type HostOs } from "../agent/toolchainGuidance";

export interface ToolchainCardOps {
  toolchainSettings(): Promise<ToolchainStatus>;
  toolchainSetEnabled(enabled: boolean): Promise<ToolchainStatus>;
  /** Stop and start the Hermes sidecar. Absent = no restart control here. */
  restartHermes?: () => Promise<void>;
}

const note: CSSProperties = { fontSize: 11.5, lineHeight: 1.6, color: "var(--tx-2)" };
const mono: CSSProperties = { fontFamily: "var(--font-mono)", fontSize: 11 };
const msg = (e: unknown) => (e instanceof Error ? e.message : String(e));

export function ToolchainCard({ ops, os = hostOs() }: { ops: ToolchainCardOps; os?: HostOs }) {
  const [tc, setTc] = useState<ToolchainStatus | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [restarted, setRestarted] = useState(false);

  const run = async (f: () => Promise<ToolchainStatus | void>) => {
    setBusy(true);
    try {
      const r = await f();
      if (r) setTc(r);
      setErr(null);
    } catch (e) {
      setErr(msg(e));
    } finally {
      setBusy(false);
    }
  };

  useEffect(() => {
    let live = true;
    ops.toolchainSettings().then((s) => live && setTc(s), (e) => live && setErr(msg(e)));
    return () => {
      live = false;
    };
  }, [ops]);

  const advice = toolchainAdvice(tc, os);

  return (
    <div data-testid="toolchain-card" style={{ display: "flex", flexDirection: "column", gap: 6 }}>
      {err && <span data-testid="cf-tc-error" style={{ ...note, color: "var(--danger)" }}>{err}</span>}
      {tc && (
        <>
          <label style={{ display: "flex", alignItems: "center", gap: 8, fontSize: 12 }}>
            <input
              type="checkbox"
              data-testid="cf-tc-toggle"
              checked={tc.settings.enabled}
              disabled={busy}
              onChange={() => void run(() => ops.toolchainSetEnabled(!tc.settings.enabled))}
            />
            Let Hermes run forge, slither, aderyn and medusa in folders you granted
          </label>
          <ul style={{ margin: 0, paddingLeft: 18 }}>
            {tc.programs.map((p) => (
              <li key={p.program} data-testid={"cf-tc-" + p.program} style={{ ...note, color: p.path ? "var(--tx-2)" : "var(--danger)" }}>
                {p.program}: {p.path ? "found" : "not installed (its gate item fails)"}
              </li>
            ))}
            <li data-testid="cf-tc-solc" style={{ ...note, color: tc.solc ? "var(--tx-2)" : "var(--danger)" }}>
              solc 0.8.36: {tc.solc ? "found" : "not found (builds fail offline)"}
            </li>
          </ul>
          {advice.kind === "windows" ? (
            <span data-testid="tc-windows" style={note}>{advice.text}</span>
          ) : advice.steps.length > 0 ? (
            <div data-testid="tc-install" style={{ display: "flex", flexDirection: "column", gap: 4 }}>
              <span style={note}>The app does not install these in 0.5.0. Install the missing ones in a terminal:</span>
              {formatSteps(advice.steps).map((line, i) => (
                <code key={advice.steps[i].program} data-testid={"tc-install-" + advice.steps[i].program} style={{ ...mono, whiteSpace: "pre-wrap", overflowWrap: "anywhere" }}>
                  {line}
                </code>
              ))}
            </div>
          ) : null}
          {tc.notices.map((n, i) => (
            <span key={i} style={note}>{n}</span>
          ))}
        </>
      )}
      <span data-testid="tc-applies" style={note}>
        Changes apply when Hermes restarts.{restarted ? " Hermes restarted." : ""}
      </span>
      <span style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
        <button className="btn btn-ghost btn-sm" data-testid="tc-recheck" disabled={busy} onClick={() => void run(() => ops.toolchainSettings())}>
          Re-check
        </button>
        {ops.restartHermes && (
          <button
            className="btn btn-ghost btn-sm"
            data-testid="tc-restart"
            disabled={busy}
            onClick={() =>
              void run(async () => {
                setRestarted(false);
                await ops.restartHermes?.();
                setRestarted(true);
              })
            }
          >
            Restart Hermes
          </button>
        )}
      </span>
    </div>
  );
}
