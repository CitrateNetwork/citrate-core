// =====================================================================
// HUP-S10.4 — passphrase panel for the encrypted journal export / import.
//
// The passphrase lives only in this component's state while the panel is open
// and is cleared after every attempt that reaches the sealer. It is never put in
// AppState or localStorage.
// =====================================================================
import { useState } from "react";
import type { JournalPage } from "../shell/state";
import { MIN_PASSPHRASE_CHARS, exportJournalEncrypted, importJournalEncrypted, type JournalIo } from "./encryptedExport";

export interface JournalVaultPanelProps {
  kind: "export" | "import";
  pages: JournalPage[];
  io: () => Promise<JournalIo>;
  now: () => Date;
  onImported: (pages: JournalPage[]) => void;
  onDone: (message: string) => void;
  onClose: () => void;
}

function plural(n: number, one: string, many: string): string {
  return `${n} ${n === 1 ? one : many}`;
}

export function JournalVaultPanel({ kind, pages, io, now, onImported, onDone, onClose }: JournalVaultPanelProps) {
  const [pass, setPass] = useState("");
  const [confirm, setConfirm] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const isExport = kind === "export";

  const go = async () => {
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      const x = await io();
      if (isExport) {
        const r = await exportJournalEncrypted(x, pages, pass, confirm, now());
        if (r.ok) {
          setPass("");
          setConfirm("");
          onDone(`Encrypted journal saved to ${r.path}`);
          onClose();
        } else if (!r.cancelled) {
          setError(r.reason);
        }
      } else {
        const r = await importJournalEncrypted(x, pages, pass);
        if (r.ok) {
          setPass("");
          onImported(r.pages);
          onDone(`Imported ${plural(r.added, "new page", "new pages")}, ${r.unchanged} already here, ${plural(r.copied, "kept as a separate copy", "kept as separate copies")}.`);
          onClose();
        } else if (!r.cancelled) {
          setError(r.reason);
        }
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="surface" style={{ display: "flex", flexDirection: "column", gap: 10, padding: "14px 18px", margin: "0 18px 14px" }}>
      <span style={{ fontSize: 13.5, fontWeight: 500 }}>{isExport ? "Export an encrypted copy" : "Import an encrypted copy"}</span>
      <span style={{ fontSize: 12, lineHeight: 1.6, color: "var(--tx-2)" }}>
        {isExport
          ? `The whole journal is sealed on this device with a passphrase you choose (at least ${MIN_PASSPHRASE_CHARS} characters) and saved where you pick. If you forget the passphrase, the file cannot be recovered.`
          : "Pick a .citrate-journal file and enter its passphrase. Pages already on this device are never overwritten; a different version of a page is added as a separate copy."}
      </span>
      <input
        data-testid="jv-pass"
        className="input"
        type="password"
        autoComplete="off"
        placeholder="Passphrase"
        value={pass}
        onChange={(e) => setPass(e.target.value)}
        style={{ height: 32 }}
      />
      {isExport && (
        <input
          data-testid="jv-confirm"
          className="input"
          type="password"
          autoComplete="off"
          placeholder="Passphrase again"
          value={confirm}
          onChange={(e) => setConfirm(e.target.value)}
          style={{ height: 32 }}
        />
      )}
      {error && (
        <span data-testid="jv-error" role="alert" style={{ fontSize: 12, color: "var(--danger)" }}>
          {error}
        </span>
      )}
      <div style={{ display: "flex", gap: 8 }}>
        <button data-testid="jv-go" className="btn btn-primary btn-sm" onClick={() => void go()} disabled={busy}>
          {busy ? "Working…" : isExport ? "Choose where to save" : "Choose a file"}
        </button>
        <button className="btn btn-ghost btn-sm" onClick={onClose} disabled={busy}>
          Cancel
        </button>
      </div>
    </div>
  );
}
