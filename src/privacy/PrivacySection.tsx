// =====================================================================
// HUP-S10.5 — Settings › Privacy & recovery.
//
// Four panels: the device-key recovery kit (phrase or file, member's choice),
// "delete my local data" (dry run, typed confirmation), what works offline (the
// offline matrix), and the default budget values (placeholders pending owner
// sign-off). Every action is real on the desktop app and says so plainly in the
// web preview (Rule 1).
// =====================================================================
import { useEffect, useState } from "react";
import type { PrivacyIo } from "./privacyIo";
import { recoveryStatus, restoreFromFile, restoreFromPhrase, saveRecoveryFile, saveRecoveryPhrase, type RecoveryStatus } from "./recovery";
import { confirmationError, deleteLocalData, formatBytes, planLocalData, type DataPlan, type DeleteOptions, type DeleteReport } from "./localData";
import matrix from "./offline-matrix.json";
import budgets from "./budget-defaults.json";

export interface PrivacySectionProps {
  io: () => Promise<PrivacyIo>;
  now: () => Date;
  toast: (m: string) => void;
}

const panel = { padding: 18, display: "flex", flexDirection: "column" as const, gap: 10 };
const note = { fontSize: 11.5, color: "var(--tx-3)", lineHeight: 1.5 };
const err = { fontSize: 12, color: "var(--err, #c0392b)", lineHeight: 1.5 };
const input = { background: "var(--srf-1)", border: "1px solid var(--line-1)", borderRadius: 6, padding: "6px 8px", fontSize: 12.5, color: "var(--tx-1)" };

// ---------------------------------------------------------------------------
// Recovery kit
// ---------------------------------------------------------------------------

export function RecoveryKitPanel({ io, now, toast }: PrivacySectionProps) {
  const [status, setStatus] = useState<RecoveryStatus | null>(null);
  const [statusError, setStatusError] = useState<string | null>(null);
  const [form, setForm] = useState<"phrase" | "file">("phrase");
  const [pass, setPass] = useState("");
  const [confirm, setConfirm] = useState("");
  const [restoreForm, setRestoreForm] = useState<"phrase" | "file">("phrase");
  const [sheet, setSheet] = useState("");
  const [restorePass, setRestorePass] = useState("");
  const [replace, setReplace] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const refresh = async () => {
    const r = await recoveryStatus(await io());
    if (r.ok) {
      setStatus(r.status);
      setStatusError(null);
    } else setStatusError(r.reason);
  };

  useEffect(() => {
    void refresh();
  }, []);

  const save = async () => {
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      const x = await io();
      const r = form === "phrase" ? await saveRecoveryPhrase(x, now()) : await saveRecoveryFile(x, pass, confirm, now());
      if (r.ok) {
        setPass("");
        setConfirm("");
        toast(
          form === "phrase"
            ? `Recovery phrase sheet saved to ${r.path}. Print it, keep it offline, then delete the file.`
            : `Recovery file saved to ${r.path}. Keep it and its passphrase somewhere safe, apart from this computer.`,
        );
        await refresh();
      } else if (!r.cancelled) setError(r.reason);
    } finally {
      setBusy(false);
    }
  };

  const restore = async () => {
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      const x = await io();
      const r = restoreForm === "phrase" ? await restoreFromPhrase(x, sheet, replace) : await restoreFromFile(x, restorePass, replace);
      if (r.ok) {
        setSheet("");
        setRestorePass("");
        setReplace(false);
        const done = r.restored.length ? `Restored: ${r.restored.join(", ")}.` : "Nothing to restore.";
        const same = r.unchanged.length ? ` Already matching: ${r.unchanged.join(", ")}.` : "";
        toast(`${done}${same} Restart the app so the node and memory store pick the keys up.`);
        await refresh();
      } else if (!r.cancelled) setError(r.reason);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="surface" style={panel} data-testid="recovery-kit">
      <span className="eyebrow">Device key recovery</span>
      <span style={{ fontSize: 12.5, lineHeight: 1.55 }}>
        This app keeps two keys of its own in your system keychain. If the keychain is lost (a new computer, a reset), the data they protect cannot be read. Back them up as a recovery phrase you print, or as a file locked with a passphrase.
      </span>
      {statusError && <span style={err}>{statusError}</span>}
      {status && !status.keyringReachable && <span style={err}>The system keychain is not reachable, so key status is unknown.</span>}
      {status && (
        <ul style={{ margin: 0, paddingLeft: 18, display: "flex", flexDirection: "column", gap: 4 }} data-testid="recovery-keys">
          {status.keys.map((k) => (
            <li key={k.account} style={{ fontSize: 12, lineHeight: 1.5 }}>
              <b>{k.label}</b>: protects {k.covers}.{" "}
              <span className="mono" style={{ color: "var(--tx-3)" }}>
                {k.present ? `present · fingerprint ${k.fingerprint ?? "?"}` : "not created yet"}
                {k.recordedFingerprint ? ` · last kit ${k.recordedFingerprint}` : " · no kit made yet"}
              </span>
            </li>
          ))}
        </ul>
      )}
      <span style={note} data-testid="recovery-scope">
        Not covered here: your wallet (it has its own recovery and is not part of this kit), your messaging key (it is re-made from your wallet), and journal export files (they open with the passphrase you chose when exporting, which nobody can recover).
      </span>

      <span style={{ fontSize: 13, fontWeight: 600, marginTop: 4 }}>Make a recovery kit</span>
      <span style={{ display: "flex", gap: 14, fontSize: 12.5 }} role="radiogroup" aria-label="Recovery kit form">
        <label>
          <input type="radio" name="kit-form" checked={form === "phrase"} onChange={() => setForm("phrase")} data-testid="kit-form-phrase" /> Recovery phrase (print it)
        </label>
        <label>
          <input type="radio" name="kit-form" checked={form === "file"} onChange={() => setForm("file")} data-testid="kit-form-file" /> Recovery file (locked with a passphrase)
        </label>
      </span>
      {form === "phrase" ? (
        <span style={note}>
          Saves a sheet with one 24-word phrase per key. The words are written straight to the file and never shown in this window. Print the sheet, keep it offline, then delete the file. Anyone with the sheet and a copy of this computer's app data can read what the keys protect.
        </span>
      ) : (
        <span style={{ display: "flex", flexDirection: "column", gap: 6 }}>
          <input type="password" autoComplete="new-password" placeholder="Passphrase (12+ characters)" value={pass} onChange={(e) => setPass(e.target.value)} style={input} data-testid="kit-pass" />
          <input type="password" autoComplete="new-password" placeholder="Passphrase again" value={confirm} onChange={(e) => setConfirm(e.target.value)} style={input} data-testid="kit-pass-confirm" />
          <span style={note}>If you forget this passphrase, the file cannot be opened. Nobody can recover it for you.</span>
        </span>
      )}
      <span>
        <button className="btn btn-secondary btn-sm" onClick={save} disabled={busy} data-testid="kit-save">
          {busy ? "Working…" : form === "phrase" ? "Save recovery phrase sheet…" : "Save recovery file…"}
        </button>
      </span>

      <span style={{ fontSize: 13, fontWeight: 600, marginTop: 4 }}>Restore from a recovery kit</span>
      <span style={{ display: "flex", gap: 14, fontSize: 12.5 }} role="radiogroup" aria-label="Restore from">
        <label>
          <input type="radio" name="restore-form" checked={restoreForm === "phrase"} onChange={() => setRestoreForm("phrase")} data-testid="restore-form-phrase" /> Recovery phrase
        </label>
        <label>
          <input type="radio" name="restore-form" checked={restoreForm === "file"} onChange={() => setRestoreForm("file")} data-testid="restore-form-file" /> Recovery file
        </label>
      </span>
      {restoreForm === "phrase" ? (
        <textarea
          rows={6}
          spellCheck={false}
          autoComplete="off"
          placeholder={"Type the sheet as printed, including each [key name] line, for example:\n[node-storage-key]\nword word word ..."}
          value={sheet}
          onChange={(e) => setSheet(e.target.value)}
          style={{ ...input, fontFamily: "var(--font-mono, monospace)" }}
          data-testid="restore-sheet"
        />
      ) : (
        <input type="password" autoComplete="off" placeholder="The recovery file's passphrase" value={restorePass} onChange={(e) => setRestorePass(e.target.value)} style={input} data-testid="restore-pass" />
      )}
      <label style={{ fontSize: 12, display: "flex", gap: 8, alignItems: "flex-start" }}>
        <input type="checkbox" checked={replace} onChange={(e) => setReplace(e.target.checked)} data-testid="restore-replace" />
        <span>Replace a different key already on this computer. Only do this when the current key was made after the loss; data written under it will no longer open.</span>
      </label>
      <span>
        <button className="btn btn-secondary btn-sm" onClick={restore} disabled={busy} data-testid="kit-restore">
          {restoreForm === "phrase" ? "Restore from phrase" : "Choose recovery file and restore…"}
        </button>
      </span>
      {error && (
        <span style={err} role="alert" data-testid="recovery-error">
          {error}
        </span>
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Delete my local data
// ---------------------------------------------------------------------------

export function DeleteLocalDataPanel({ io }: PrivacySectionProps) {
  const [opts, setOpts] = useState<DeleteOptions>({ includeWallet: false, keepModels: false });
  const [plan, setPlan] = useState<DataPlan | null>(null);
  const [confirm, setConfirm] = useState("");
  const [walletConfirm, setWalletConfirm] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [report, setReport] = useState<DeleteReport | null>(null);
  const [busy, setBusy] = useState(false);

  const choose = (patch: Partial<DeleteOptions>) => {
    setOpts((o) => ({ ...o, ...patch }));
    setPlan(null); // a changed choice needs a fresh dry run
  };

  const dryRun = async () => {
    setBusy(true);
    setError(null);
    try {
      const r = await planLocalData(await io(), opts);
      if (r.ok) setPlan(r.plan);
      else setError(r.reason);
    } finally {
      setBusy(false);
    }
  };

  const del = async () => {
    setBusy(true);
    setError(null);
    try {
      const r = await deleteLocalData(await io(), opts, confirm, walletConfirm);
      if (r.ok) setReport(r.report);
      else setError(r.reason);
    } finally {
      setBusy(false);
    }
  };

  const blocked = confirmationError(opts, confirm, walletConfirm) !== null;

  if (report) {
    return (
      <div className="surface" style={panel} data-testid="delete-data">
        <span className="eyebrow">Delete my local data</span>
        <span style={{ fontSize: 12.5 }} data-testid="delete-report">
          Deleted {report.deleted.length} items and {report.keychainDeleted.length} keychain entries. The app closes in a moment.
          {report.failed.length + report.keychainFailed.length > 0 && ` ${report.failed.length + report.keychainFailed.length} could not be deleted:`}
        </span>
        {[...report.failed, ...report.keychainFailed].map((f) => (
          <span key={f.item} className="mono" style={err}>
            {f.item}: {f.error}
          </span>
        ))}
        <span style={note}>To finish uninstalling, move the app to the Trash (macOS) or uninstall it from your system settings.</span>
      </div>
    );
  }

  return (
    <div className="surface" style={panel} data-testid="delete-data">
      <span className="eyebrow">Delete my local data</span>
      <span style={{ fontSize: 12.5, lineHeight: 1.55 }}>
        Removes this app's folders (settings, node data, memory, journal, models, caches and logs) and the keychain entries it owns, then closes the app. Use it before uninstalling. Nothing on the chain changes.
      </span>
      <label style={{ fontSize: 12.5, display: "flex", gap: 8 }}>
        <input type="checkbox" checked={opts.keepModels} onChange={(e) => choose({ keepModels: e.target.checked })} data-testid="opt-keep-models" />
        Keep downloaded models
      </label>
      <label style={{ fontSize: 12.5, display: "flex", gap: 8, alignItems: "flex-start" }}>
        <input type="checkbox" checked={opts.includeWallet} onChange={(e) => choose({ includeWallet: e.target.checked })} data-testid="opt-include-wallet" />
        <span>
          Also delete my wallet. <b>This cannot be undone.</b> Without your wallet's own recovery, any funds and membership on it are lost for good.
        </span>
      </label>
      <span>
        <button className="btn btn-secondary btn-sm" onClick={dryRun} disabled={busy} data-testid="delete-dry-run">
          {busy && !plan ? "Checking…" : "Show what would be deleted (dry run)"}
        </button>
      </span>
      {plan && (
        <div data-testid="delete-plan" style={{ display: "flex", flexDirection: "column", gap: 6 }}>
          <span style={{ fontSize: 12.5 }}>
            Deletes {formatBytes(plan.deleteBytes)}; keeps {formatBytes(plan.keepBytes)}.
          </span>
          <div style={{ maxHeight: 220, overflow: "auto", border: "1px solid var(--line-1)", borderRadius: 6, padding: "6px 8px" }}>
            {plan.entries.map((e) => (
              <div key={e.path} className="mono" style={{ fontSize: 11, lineHeight: 1.6 }}>
                {e.action === "delete" ? "delete" : "keep  "} {formatBytes(e.bytes).padStart(9)} {e.path}
                {e.reason ? ` (${e.reason})` : ""}
              </div>
            ))}
            {plan.keychain.map((k) => (
              <div key={k.service + "/" + k.account} className="mono" style={{ fontSize: 11, lineHeight: 1.6 }}>
                {k.action === "delete" ? "delete" : "keep  "} keychain {k.service} / {k.account} ({k.label}
                {k.present === null ? ", presence unknown" : k.present ? "" : ", not present"})
              </div>
            ))}
          </div>
          {plan.notes.map((n) => (
            <span key={n} style={note}>
              Note: {n}
            </span>
          ))}
          <input placeholder={`Type "${plan.confirmPhrase}"`} value={confirm} onChange={(e) => setConfirm(e.target.value)} style={input} data-testid="delete-confirm" autoComplete="off" />
          {plan.walletConfirmPhrase && (
            <input placeholder={`Type "${plan.walletConfirmPhrase}"`} value={walletConfirm} onChange={(e) => setWalletConfirm(e.target.value)} style={input} data-testid="delete-wallet-confirm" autoComplete="off" />
          )}
          <span>
            <button className="btn btn-primary btn-sm" onClick={del} disabled={busy || blocked} data-testid="delete-go">
              Delete my local data
            </button>
          </span>
        </div>
      )}
      {error && (
        <span style={err} role="alert" data-testid="delete-error">
          {error}
        </span>
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Offline matrix + budget defaults (read-only)
// ---------------------------------------------------------------------------

const OFFLINE_LABEL: Record<string, string> = { works: "works offline", degrades: "limited offline", unavailable: "needs the network" };

export function OfflineMatrixPanel() {
  return (
    <div className="surface" style={panel} data-testid="offline-matrix">
      <span className="eyebrow">What works offline</span>
      {matrix.features.map((f) => (
        <div key={f.id} style={{ display: "flex", flexDirection: "column", gap: 2 }}>
          <span style={{ fontSize: 12.5 }}>
            <b>{f.feature}</b> <span className="mono" style={{ color: "var(--tx-3)", fontSize: 11 }}>{OFFLINE_LABEL[f.offline] ?? f.offline}</span>
          </span>
          <span style={note}>{f.behaviour}</span>
        </div>
      ))}
    </div>
  );
}

export function BudgetDefaultsPanel() {
  const rows: [string, string][] = [
    ["Sign-in with Ethereum, per site: sign-ins per budget", String(budgets.siwe.maxCount)],
    ["Sign-in with Ethereum: budget lifetime", `${budgets.siwe.expiresDays} days`],
    ["x402 payment: largest single payment", `${budgets.x402.perSignatureMaxSalt} SALT`],
    ["x402 payment: per recipient, rolling 24 h", `${budgets.x402.perRecipientWindowMaxSalt} SALT`],
    ["x402 payment: all recipients, rolling 24 h", `${budgets.x402.globalWindowMaxSalt} SALT`],
    ["x402 payment: payments per budget", String(budgets.x402.maxCount)],
    ["x402 payment: budget lifetime", `${budgets.x402.expiresDays} day${budgets.x402.expiresDays === 1 ? "" : "s"}`],
    ["Scheduled tasks: runs per day", String(budgets.daemons.maxRunsPerDay)],
    ["Scheduled tasks: minutes per run", String(budgets.daemons.maxMinutesPerRun)],
    ["Scheduled tasks: model tokens per run", budgets.daemons.maxModelTokensPerRun.toLocaleString("en-US")],
  ];
  return (
    <div className="surface" style={panel} data-testid="budget-defaults">
      <span className="eyebrow">Default budget values · pending owner sign-off</span>
      <span style={note}>
        Starting values for a budget you choose to grant. No budget exists until you grant one through an approval card. x402 payments stay off until a payment token is deployed on chain.
      </span>
      {rows.map(([k, v]) => (
        <span key={k} style={{ display: "flex", justifyContent: "space-between", gap: 12, fontSize: 12 }}>
          <span>{k}</span>
          <span className="mono">{v}</span>
        </span>
      ))}
    </div>
  );
}

export function PrivacySection(props: PrivacySectionProps) {
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 12 }} data-testid="privacy-section">
      <RecoveryKitPanel {...props} />
      <DeleteLocalDataPanel {...props} />
      <OfflineMatrixPanel />
      <BudgetDefaultsPanel />
    </div>
  );
}
