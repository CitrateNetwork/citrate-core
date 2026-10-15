// =====================================================================
// HGW-W11 — Settings > Hermes · building: the auto-approve switch, the list in effect (read from
// the running Hermes), and the member's name and email for Hermes's commits.
// =====================================================================
import { useEffect, useState } from "react";
import {
  AUTO_APPROVE_HINT,
  AUTO_APPROVE_LABEL,
  IDENTITY_HINT,
  readAutoApproveList,
  readBuildSettings,
  ruleText,
  writeBuildSettings,
  type AutoApproveListView,
  type BuildSettingsIo,
  type BuildSettingsStatus,
} from "./buildSettings";

export interface BuildSettingsPanelProps {
  io: () => Promise<BuildSettingsIo>;
  /** Called when a change restarted Hermes (the chat then opens a fresh conversation). */
  onRestarted?: () => void;
}

const msg = (e: unknown) => (e instanceof Error ? e.message : String(e));

export function BuildSettingsPanel({ io, onRestarted }: BuildSettingsPanelProps) {
  const [x, setX] = useState<BuildSettingsIo | null>(null);
  const [st, setSt] = useState<BuildSettingsStatus | null>(null);
  const [list, setList] = useState<AutoApproveListView | null>(null);
  const [listError, setListError] = useState<string | null>(null);
  const [name, setName] = useState("");
  const [email, setEmail] = useState("");
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  async function refreshList(got: BuildSettingsIo) {
    try {
      setList(await readAutoApproveList(got));
      setListError(null);
    } catch (e) {
      setList(null);
      setListError(msg(e));
    }
  }

  useEffect(() => {
    let live = true;
    void (async () => {
      try {
        const got = await io();
        if (!live) return;
        setX(got);
        const s = await readBuildSettings(got);
        if (!live || !s) return;
        setSt(s);
        setName(s.gitName ?? "");
        setEmail(s.gitEmail ?? "");
        await refreshList(got);
      } catch (e) {
        if (live) setError(msg(e));
      }
    })();
    return () => {
      live = false;
    };
  }, [io]);

  if (x && x.mode !== "tauri") {
    return <span style={{ fontSize: 12, color: "var(--tx-3)" }}>Hermes builds in the desktop app.</span>;
  }

  async function save(autoApprove: boolean) {
    if (!x) return;
    setBusy(true);
    setError(null);
    setNote(null);
    try {
      const next = await writeBuildSettings(x, autoApprove, name, email);
      setSt(next);
      setName(next.gitName ?? "");
      setEmail(next.gitEmail ?? "");
      setNote(next.restarted ? "Saved. Hermes restarted, so new conversations use it." : "Saved.");
      if (next.restarted) {
        onRestarted?.();
        await refreshList(x);
      }
    } catch (e) {
      setError(msg(e));
    } finally {
      setBusy(false);
    }
  }

  const on = st?.autoApprove ?? false;
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 10 }} data-testid="build-settings">
      {st?.loadError ? <span style={{ fontSize: 12, color: "var(--warn)" }}>{st.loadError}</span> : null}
      <label style={{ display: "flex", alignItems: "center", gap: 10, fontSize: 13 }}>
        <input id="hermes-auto-approve" type="checkbox" checked={on} disabled={!st || busy} onChange={(e) => void save(e.target.checked)} />
        {AUTO_APPROVE_LABEL}
      </label>
      <span style={{ fontSize: 11.5, color: "var(--tx-3)", lineHeight: 1.5 }}>{AUTO_APPROVE_HINT}</span>
      {list ? (
        <div style={{ fontSize: 12, lineHeight: 1.6 }} data-testid="auto-approve-list">
          {list.enabled ? (
            <>
              <span style={{ color: "var(--tx-2)" }}>Runs without a card, when all of these hold:</span>
              <ul style={{ margin: "2px 0 6px 18px", padding: 0 }}>
                {list.conditions.map((c) => (
                  <li key={c}>{c}</li>
                ))}
              </ul>
              <ul style={{ margin: "0 0 0 18px", padding: 0, fontFamily: "var(--mono)" }}>
                {list.rules.map((r) => (
                  <li key={r.id}>{ruleText(r)}</li>
                ))}
              </ul>
            </>
          ) : (
            <span style={{ color: "var(--tx-3)" }}>Off in the running Hermes: every command asks you.</span>
          )}
        </div>
      ) : listError ? (
        <span style={{ fontSize: 12, color: "var(--tx-3)" }}>The list in effect is not available: {listError}</span>
      ) : null}
      <span className="eyebrow" style={{ marginTop: 6 }}>
        Commit identity
      </span>
      <div style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
        <input aria-label="Name for Hermes's commits" placeholder="Name" value={name} onChange={(e) => setName(e.target.value)} disabled={!st || busy} />
        <input aria-label="Email for Hermes's commits" placeholder="Email" value={email} onChange={(e) => setEmail(e.target.value)} disabled={!st || busy} />
        <button className="btn" disabled={!st || busy} onClick={() => void save(on)}>
          Save
        </button>
      </div>
      <span style={{ fontSize: 11.5, color: "var(--tx-3)", lineHeight: 1.5 }}>{IDENTITY_HINT}</span>
      {note ? <span style={{ fontSize: 12, color: "var(--tx-2)" }}>{note}</span> : null}
      {error ? <span style={{ fontSize: 12, color: "var(--warn)" }}>{error}</span> : null}
    </div>
  );
}
