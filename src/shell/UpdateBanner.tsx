// =====================================================================
// citrate-core — update affordance (W2.4)
//
// A non-blocking, dismissible card (bottom-right) that surfaces a real available
// update and drives download → restart. Never modal, never blocks the app.
// Renders nothing at all unless there is genuinely something to show, so in
// sim/web (and when up to date) it is invisible.
//
// 0.5.4 exception: below a VERIFIED minimum version (updater.ts `required`), App renders only
// UpdateGate: a modal dialog (focus trapped, app shortcuts blocked) holding this card inline, so
// nothing of the app is focusable or clickable. The card cannot be dismissed, and the update
// downloads by itself. Only a minimum signed with the updater key does this.
// =====================================================================
import { useEffect, useState, type MouseEvent, type ReactNode } from "react";
import { useAppUpdate, formatBytes, progressFraction, type UpdateState } from "./updater";
import { ModalDialog } from "./ModalDialog";
import { bridge } from "../bridge";

/** #678 review: an external link opens in the system browser through core's `open_external`
 *  (a plain target=_blank anchor does not reliably leave the Tauri webview). The URL stays visible
 *  as the link text, so it can be typed by hand if opening fails. */
export function ExternalLink({ url, children }: { url: string; children?: string }) {
  const onClick = (e: MouseEvent<HTMLAnchorElement>) => {
    e.preventDefault();
    void bridge.shell.openExternal(url).catch(() => undefined);
  };
  return (
    <a href={url} onClick={onClick} rel="noreferrer">
      {children ?? url.replace(/^https:\/\//, "")}
    </a>
  );
}

/** Keys that stay usable while gated: copy and select-all (for the support address). */
const GATE_ALLOWED_MOD_KEYS = new Set(["c", "a"]);

/**
 * #678 review (P1): while an update is required no app shortcut may fire. A capture-phase listener
 * on window runs before every other keydown listener (window, document or element), so a
 * modifier shortcut such as Cmd/Ctrl+B is stopped there. Plain keys (Tab, Enter, Space) still reach
 * the dialog. Returns the unsubscribe.
 */
export function blockShortcutsWhileGated(target: Window = window): () => void {
  const onKey = (e: KeyboardEvent) => {
    if (!(e.metaKey || e.ctrlKey || e.altKey)) return;
    if ((e.metaKey || e.ctrlKey) && !e.altKey && GATE_ALLOWED_MOD_KEYS.has(e.key.toLowerCase())) return;
    e.preventDefault();
    e.stopImmediatePropagation();
  };
  target.addEventListener("keydown", onKey, true);
  return () => target.removeEventListener("keydown", onKey, true);
}

/** #678 review: where a gated member can get the update by hand. Installers are behind the invite
 *  gate, so this is the download page (not an installer URL); support sends a fresh invite link. */
export const MANUAL_DOWNLOAD_URL = "https://www.citrate.ai/download";
export const SUPPORT_EMAIL = "support@citrate.ai";

/** Quit the app (the gated member's way out). */
async function quitApp(): Promise<void> {
  try {
    const { exit } = await import("@tauri-apps/plugin-process");
    await exit(0);
  } catch {
    window.close();
  }
}

/** Copy `text`; resolves to whether it worked (no clipboard in some webviews). */
async function copyText(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    return false;
  }
}

/** The way out when this app cannot install the update itself: quit, download by hand, support. */
export function UpdateRequiredHelp({
  error,
  onQuit = () => void quitApp(),
  onCopy = copyText,
}: {
  error: string | null;
  onQuit?: () => void;
  onCopy?: (text: string) => Promise<boolean>;
}) {
  const [copied, setCopied] = useState<null | boolean>(null);
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 10, borderTop: "1px solid var(--line-1, rgba(127,127,127,.25))", paddingTop: 12 }}>
      {error && (
        <div aria-label="Update error">
          <span style={{ display: "block", fontSize: 11.5, color: "var(--tx-3)" }}>What went wrong</span>
          <pre className="mono" style={{ margin: 0, fontSize: 11, color: "var(--danger)", whiteSpace: "pre-wrap", wordBreak: "break-word", userSelect: "text" }}>
            {error}
          </pre>
        </div>
      )}
      <div aria-label="Download manually">
        <span style={{ display: "block", fontSize: 13, fontWeight: 600 }}>Download manually</span>
        <p style={{ margin: "4px 0 0", fontSize: 12.5, lineHeight: 1.55, color: "var(--tx-2)" }}>
          If the update will not install here, quit Citrate Core and install the new version from{" "}
          <ExternalLink url={MANUAL_DOWNLOAD_URL} />
          . Downloads need an invite link; if yours no longer works, ask support for a fresh one. Installing over this version keeps your wallet, keys and data.
        </p>
      </div>
      <div style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap" }}>
        <span style={{ fontSize: 12.5, color: "var(--tx-2)" }}>Support:</span>
        <span className="mono" style={{ fontSize: 12.5, userSelect: "all" }}>
          {SUPPORT_EMAIL}
        </span>
        <button className="btn btn-ghost btn-sm" onClick={() => void onCopy(SUPPORT_EMAIL).then(setCopied)}>
          {copied === true ? "Copied" : "Copy"}
        </button>
        {copied === false && <span style={{ fontSize: 11.5, color: "var(--tx-3)" }}>Copy did not work here; select the address instead.</span>}
      </div>
      <div>
        <button className="btn btn-ghost btn-sm" onClick={onQuit}>
          Quit Citrate Core
        </button>
      </div>
    </div>
  );
}

/** 0.5.4: the blocking screen while this version is below the verified minimum. */
export function UpdateRequiredScreen({
  state,
  onRetry,
  onQuit,
  onCopy,
  install,
  restart,
}: {
  state: UpdateState;
  onRetry: () => void;
  onQuit?: () => void;
  onCopy?: (text: string) => Promise<boolean>;
  /** The update card's actions; the card is drawn inside the dialog (#678 review P1). */
  install?: () => Promise<void> | void;
  restart?: () => Promise<void> | void;
}) {
  // No app shortcut fires while this screen is up.
  useEffect(() => (state.required ? blockShortcutsWhileGated() : undefined), [state.required]);
  if (!state.required) return null;
  const noUpdate = state.status === "uptodate" || state.status === "idle" || state.status === "checking";
  return (
    <ModalDialog
      labelledBy="update-required-title"
      focusKey="update-required"
      register="charter"
      zIndex={59}
      panelStyle={{ maxWidth: 480, width: "100%", maxHeight: "100%", overflowY: "auto", background: "var(--srf-0)", color: "var(--tx-1)", borderRadius: 12, padding: 20 }}
    >
      <div data-update-gate="" style={{ display: "flex", flexDirection: "column", gap: 12 }}>
        <span id="update-required-title" style={{ fontSize: 18, fontWeight: 600 }}>
          Update required{state.minimumVersion ? `: Citrate Core ${state.minimumVersion} or later` : ""}
        </span>
        <p style={{ margin: 0, fontSize: 13, lineHeight: 1.6, color: "var(--tx-2)" }}>
          This version can no longer be used. {state.requiredReason ? `${state.requiredReason}. ` : ""}
          {state.status === "ready"
            ? "The update is downloaded: restart to install it."
            : state.status === "downloading"
              ? "The update is downloading."
              : state.status === "installing"
                ? "Installing the update."
                : state.status === "error"
                  ? "The update could not be downloaded."
                  : noUpdate
                    ? "No update could be found right now. Check your connection; the app looks again every few minutes."
                    : "The update downloads by itself."}
        </p>
        <p style={{ margin: 0, fontSize: 11.5, color: "var(--tx-3)", lineHeight: 1.5 }}>
          Your wallet, keys and data stay on this device and are kept by the update.
        </p>
        {(noUpdate || state.status === "error") && (
          <div>
            <button className="btn btn-primary btn-sm" onClick={onRetry}>
              Check for the update
            </button>
          </div>
        )}
        <UpdateCard state={state} install={install ?? (() => undefined)} restart={restart ?? (() => undefined)} dismiss={() => undefined} inline />
        <UpdateRequiredHelp error={state.error} onQuit={onQuit} onCopy={onCopy} />
      </div>
    </ModalDialog>
  );
}

export type AppUpdate = ReturnType<typeof useAppUpdate>;

/** The corner card (not gated). While an update is required App renders `UpdateGate` instead of
 *  the whole app, so this renders nothing then. */
export function UpdateBanner({ update }: { update: AppUpdate }) {
  const { state, install, restart, dismiss } = update;
  if (state.required) return null;
  return <UpdateCard state={state} install={install} restart={restart} dismiss={dismiss} />;
}

/** #678 review P1: the app (`children`) or, while an update is required, only the gate. The app is
 *  not rendered at all then: no control of it exists to focus or click. */
export function GateOrApp({ update, children }: { update: AppUpdate; children: ReactNode }) {
  return update.state.required ? <UpdateGate update={update} /> : <>{children}</>;
}

/** #678 review P1: the whole window while an update is required (App renders nothing else, so no
 *  control of the app is focusable or clickable behind it). */
export function UpdateGate({ update }: { update: AppUpdate }) {
  const { state, check, install, restart } = update;
  return <UpdateRequiredScreen state={state} onRetry={() => void check()} install={install} restart={restart} />;
}

/** The card itself (state from useAppUpdate). */
export function UpdateCard({
  state,
  install,
  restart,
  dismiss,
  inline = false,
}: {
  state: UpdateState;
  install: () => Promise<void> | void;
  restart: () => Promise<void> | void;
  dismiss: () => void;
  /** Drawn in the flow (inside the gate) instead of fixed in the corner. */
  inline?: boolean;
}) {
  const visible =
    !state.dismissed &&
    (state.status === "available" ||
      state.status === "downloading" ||
      state.status === "ready" ||
      state.status === "installing" ||
      state.status === "installed" ||
      state.status === "error");
  if (!visible) return null;

  const accent = state.critical || state.required ? "var(--danger)" : "var(--acc-1, var(--tx-1))";
  const frac = progressFraction(state);

  return (
    <div
      role="status"
      aria-live="polite"
      style={{
        ...(inline ? {} : { position: "fixed" as const, right: 20, bottom: 20, width: 340, zIndex: 60 }),
        background: "var(--srf-1)",
        border: "1px solid var(--bd-1)",
        borderLeft: `3px solid ${accent}`,
        borderRadius: 10,
        boxShadow: "0 10px 30px rgba(0,0,0,0.28)",
        padding: "14px 16px",
        display: "flex",
        flexDirection: "column",
        gap: 10,
        color: "var(--tx-1)",
      }}
    >
      <div style={{ display: "flex", alignItems: "baseline", gap: 8 }}>
        <span style={{ fontSize: 13.5, fontWeight: 600, flex: 1 }}>
          {state.status === "ready"
            ? "Update ready to install"
            : state.status === "installing"
              ? "Stopping services and installing…"
              : state.status === "installed"
                ? "Restart Citrate Core to finish the update"
            : state.status === "downloading"
              ? "Downloading update…"
              : state.status === "error"
                ? "Update failed"
                : state.required
                  ? "Required update"
                  : state.critical
                  ? "Critical update available"
                  : "Update available"}
        </span>
        {state.version && (
          <span className="mono" style={{ fontSize: 11, color: "var(--tx-3)" }}>
            v{state.version}
          </span>
        )}
      </div>

      {state.status === "error" || state.status === "installed" ? (
        <p style={{ margin: 0, fontSize: 12, color: "var(--danger)", lineHeight: 1.5 }}>
          {state.error || "Could not download the update. Check your connection and try again."}
        </p>
      ) : state.status === "downloading" ? (
        <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
          <div style={{ height: 6, borderRadius: 999, background: "var(--srf-2, rgba(127,127,127,.18))", overflow: "hidden" }}>
            <div
              style={{
                height: "100%",
                width: frac === null ? "40%" : `${Math.round(frac * 100)}%`,
                background: accent,
                borderRadius: 999,
                transition: "width .2s linear",
                // indeterminate shimmer when the server sent no content-length
                opacity: frac === null ? 0.6 : 1,
              }}
            />
          </div>
          <span className="mono" style={{ fontSize: 10.5, color: "var(--tx-3)" }}>
            {formatBytes(state.downloadedBytes)}
            {state.totalBytes ? ` / ${formatBytes(state.totalBytes)}` : ""}
          </span>
        </div>
      ) : (
        state.notes && (
          <p style={{ margin: 0, fontSize: 12, color: "var(--tx-2)", lineHeight: 1.5, maxHeight: 66, overflow: "hidden" }}>
            {state.notes.replace(/\[critical\]/gi, "").trim()}
          </p>
        )
      )}

      <div style={{ display: "flex", gap: 8, justifyContent: "flex-end" }}>
        {state.status !== "downloading" && state.status !== "installing" && !state.required && (
          <button className="btn btn-ghost btn-sm" onClick={dismiss}>
            {state.status === "ready" ? "Later" : "Dismiss"}
          </button>
        )}
        {state.status === "available" && (
          <button className="btn btn-primary btn-sm" onClick={() => void install()}>
            Download &amp; install
          </button>
        )}
        {state.status === "ready" && (
          <button className="btn btn-primary btn-sm" onClick={() => void restart()}>
            Restart now
          </button>
        )}
        {state.status === "error" && (
          <button className="btn btn-primary btn-sm" onClick={() => void install()}>
            Retry
          </button>
        )}
      </div>
    </div>
  );
}
