// =====================================================================
// citrate-core — update affordance (W2.4)
//
// A non-blocking, dismissible card (bottom-right) that surfaces a real available
// update and drives download → restart. Never modal, never blocks the app.
// Renders nothing at all unless there is genuinely something to show, so in
// sim/web (and when up to date) it is invisible.
// =====================================================================
import { useAppUpdate, formatBytes, progressFraction } from "./updater";

export function UpdateBanner() {
  const { state, install, restart, dismiss } = useAppUpdate();

  const visible =
    !state.dismissed &&
    (state.status === "available" ||
      state.status === "downloading" ||
      state.status === "ready" ||
      state.status === "error");
  if (!visible) return null;

  const accent = state.critical ? "var(--danger)" : "var(--acc-1, var(--tx-1))";
  const frac = progressFraction(state);

  return (
    <div
      role="status"
      aria-live="polite"
      style={{
        position: "fixed",
        right: 20,
        bottom: 20,
        width: 340,
        zIndex: 60,
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
            : state.status === "downloading"
              ? "Downloading update…"
              : state.status === "error"
                ? "Update failed"
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

      {state.status === "error" ? (
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
        {state.status !== "downloading" && (
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
