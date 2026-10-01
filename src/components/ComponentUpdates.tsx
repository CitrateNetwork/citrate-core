// HUP-S5.5 / S6.1: Settings card for signed first-run components (the dApp toolchain today;
// the managed browser, search, skills and docs graph as they land).
//
// Honest by construction: the status comes from Rust `components_status` (read-only). While the
// component signing key slot is empty (the key ceremony has not happened) every install button
// is disabled and the card says why. The CVE SLA numbers carry "pending owner sign-off" until
// the owner signs them off.
import { useEffect, useState } from "react";
import { bridge } from "../bridge";
import type { ComponentsStatus } from "../bridge/domains";

export function platformStateLabel(state: string): string {
  switch (state) {
    case "measured":
      return "hash measured";
    case "to_be_measured":
      return "hash not measured yet";
    case "to_be_built":
      return "package not built yet";
    case "upstream_unavailable":
      return "no upstream build for this machine";
    default:
      return "not listed for this machine";
  }
}

function freshnessText(s: ComponentsStatus): string {
  switch (s.freshness) {
    case "never_checked":
      return "No component manifest has been checked on this machine yet.";
    case "fresh":
      return "Component updates are current.";
    case "stale":
      return "Component updates are stale: the last signed manifest is more than " + s.sla.staleAfterDays + " days old.";
    default:
      return "The last signed component manifest has expired.";
  }
}

const note = { fontSize: 11.5, color: "var(--tx-3)", lineHeight: 1.5 } as const;

export function ComponentUpdatesView({
  status,
  error,
  busy,
  onUpdate,
  onRollback,
}: {
  status: ComponentsStatus | null;
  error: string | null;
  busy: string | null;
  onUpdate: (name: string) => void;
  onRollback: (name: string) => void;
}) {
  if (!status) {
    return (
      <span style={note}>
        {error ? "Component status is not available: " + error : "Component updates need the desktop app."}
      </span>
    );
  }
  const sla = status.sla;
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
      <span style={{ fontSize: 13 }}>
        {status.keyConfigured
          ? "Signed with the component key " + (status.keyFingerprint ?? "") + "."
          : "Updates are off. The component signing key is set at a key ceremony that has not happened yet, so nothing is downloaded or installed."}
      </span>
      <span style={note}>{freshnessText(status)}</span>
      <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
        {status.bundle.map((t) => {
          const installable = status.keyConfigured && t.thisPlatform === "measured";
          return (
            <div key={t.name} style={{ display: "flex", alignItems: "center", gap: 10, fontSize: 12 }}>
              <span className="mono" style={{ minWidth: 150 }}>
                {t.name} {t.version}
              </span>
              <span className="mono" style={{ color: "var(--tx-3)", minWidth: 130 }}>
                {t.license}
              </span>
              <span style={{ color: "var(--tx-2)", flex: 1 }}>{platformStateLabel(t.thisPlatform)}</span>
              <button
                className="btn btn-ghost btn-sm"
                disabled={!installable || busy !== null}
                onClick={() => onUpdate(t.name)}
              >
                {busy === t.name ? "Installing " + t.name : "Install " + t.name}
              </button>
            </div>
          );
        })}
      </div>
      {status.installed.length === 0 ? (
        <span style={note}>Nothing is installed yet.</span>
      ) : (
        status.installed.map((c) => (
          <span key={c.name} style={{ display: "flex", alignItems: "center", gap: 10, fontSize: 12 }}>
            <span className="mono">
              {c.name} {c.version} installed
            </span>
            {c.previous && (
              <button className="btn btn-ghost btn-sm" disabled={busy !== null} onClick={() => onRollback(c.name)}>
                Roll back {c.name} to {c.previous}
              </button>
            )}
          </span>
        ))
      )}
      <span style={note}>
        Security fixes for bundled components: critical within {sla.criticalHours} h, high within {sla.highDays} days,
        medium within {sla.mediumDays} days, low within {sla.lowDays} days.
        {sla.pendingOwnerSignoff ? " These values are placeholders, pending owner sign-off." : ""} The managed browser stays
        off the open web until a current signed manifest has been checked.
      </span>
      {error && <span style={{ ...note, color: "var(--warn)" }}>{error}</span>}
    </div>
  );
}

export function ComponentUpdates({ toast }: { toast?: (m: string) => void }) {
  const [status, setStatus] = useState<ComponentsStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const say = toast ?? ((m: string) => console.log("[components]", m));

  const load = async () => {
    try {
      setStatus(await bridge.components.status());
      setError(null);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  useEffect(() => {
    void load();
  }, []);

  const run = async (name: string, op: (n: string) => Promise<string>) => {
    setBusy(name);
    try {
      say(await op(name));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(null);
      await load();
    }
  };

  return (
    <ComponentUpdatesView
      status={status}
      error={error}
      busy={busy}
      onUpdate={(n) => void run(n, (x) => bridge.components.update(x))}
      onRollback={(n) => void run(n, (x) => bridge.components.rollback(x))}
    />
  );
}
