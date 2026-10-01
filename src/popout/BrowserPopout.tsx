// =====================================================================
// citrate-core — the Browser pop-out view (HUP-S5.1 + S5.6, US-5.1 "Watch it browse")
//
// A pure view over one BrowserView: the screencast of Hermes's browser with the element Hermes is
// about to act on (or just acted on) outlined, the page address, the mode (managed browser or the
// member's own Chrome), and a Stop button that is always visible. Honest states for everything
// else: browser off, Hermes not running, no Chromium installed, stopped, a site without consent
// (its frames are withheld), an action waiting for the member. Decisions and consent are taken in
// the main window only; this window holds no app commands.
// =====================================================================
import type { CSSProperties } from "react";
import { frameSrc, highlightBox, type BrowserView } from "./browserView";

const label: CSSProperties = { fontSize: 9.5, letterSpacing: ".13em", textTransform: "uppercase", color: "var(--tx-3)" };
const note: CSSProperties = { fontSize: 12, color: "var(--tx-2)", lineHeight: 1.45 };
const banner: CSSProperties = { fontSize: 12, lineHeight: 1.45, padding: "8px 10px", borderRadius: 6, border: "1px solid var(--line-2)", background: "var(--srf-1)" };

function modeText(v: BrowserView): string {
  const s = v.state;
  if (s.mode === "managed") return "Managed browser";
  if (s.mode === "attached") return s.attachPort !== null ? `Your Chrome (port ${s.attachPort})` : "Your Chrome";
  return "Not running";
}

/** The one sentence that explains an empty screen, or null when there is a page to show. */
function stateNote(v: BrowserView): string | null {
  const s = v.state;
  if (!s.enabled) {
    if (s.running === false) return "Hermes is not running. Start it from the Agent section, then come back.";
    return "Hermes's browser is off in this build. Nothing is browsing.";
  }
  if (s.stopped) return "Stopped. Nothing will browse until you resume it from the main window.";
  if (s.mode === "off" && s.chromium?.state === "not_installed") {
    return "No Chromium is installed. The managed browser arrives with the component updater; until then Hermes uses Chrome, Chromium, Edge or Brave if one is installed.";
  }
  if (s.mode === "off") return "Hermes has not opened a page yet. It starts its browser when a task needs one.";
  return null;
}

export function BrowserPopout({ view, onStop }: { view: BrowserView; onStop: () => void }) {
  const s = view.state;
  const src = frameSrc(view.frame);
  const box = highlightBox(view.frame);
  const active = s.enabled && !s.stopped && s.mode !== "off";
  const msg = stateNote(view);
  const categoryLabel = (id: string | null) => (id ? s.excludedCategories.find((c) => c.id === id)?.label ?? id : null);

  return (
    <div
      data-testid="browser-popout"
      data-register="instrument"
      style={{ minHeight: "100vh", boxSizing: "border-box", padding: "12px 14px", background: "var(--srf-0)", color: "var(--tx-1)", fontFamily: "var(--font-sans)", display: "flex", flexDirection: "column", gap: 8 }}
    >
      <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
        <span style={{ fontSize: 14, fontWeight: 500 }}>Browser</span>
        <span className="mono" data-testid="browser-mode" style={label}>
          {modeText(view)}
        </span>
        <span style={{ flex: 1 }} />
        <button
          className="btn btn-sm"
          data-testid="browser-stop"
          onClick={onStop}
          disabled={!active}
          title={active ? "Stop Hermes's browser now" : "Nothing is browsing"}
          style={{ color: active ? "var(--danger)" : "var(--tx-3)", borderColor: active ? "var(--danger)" : "var(--line-2)", background: "transparent" }}
        >
          Stop
        </button>
      </div>

      {s.url ? (
        <div className="mono" data-testid="browser-url" title={s.url} style={{ fontSize: 11, color: "var(--tx-2)", padding: "5px 8px", border: "1px solid var(--line-1)", borderRadius: 6, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
          {s.url}
        </div>
      ) : null}

      {s.pendingAction ? (
        <div data-testid="browser-pending" role="status" style={{ ...banner, borderColor: "var(--accent)" }}>
          <strong>Hermes is waiting for your decision:</strong> {s.pendingAction.summary}. Decide in the main window.
        </div>
      ) : null}

      {s.consentNeeded ? (
        <div data-testid="browser-consent" role="status" style={banner}>
          {s.consentNeeded.category
            ? `${s.consentNeeded.origin} is a ${categoryLabel(s.consentNeeded.category)} site, excluded by default. Hermes will not use it unless you include it in the main window.`
            : `${s.consentNeeded.origin} needs your consent before Hermes can use it in your Chrome. You can allow it in the main window.`}
        </div>
      ) : null}

      {msg ? (
        <div data-testid="browser-note" style={note}>
          {msg}
        </div>
      ) : null}

      {view.frame?.withheld ? (
        <div data-testid="browser-withheld" style={{ ...note, padding: 24, textAlign: "center", border: "1px dashed var(--line-2)", borderRadius: 6 }}>
          Hidden: this site does not have your consent, so its picture is not shown here.
        </div>
      ) : src ? (
        <div style={{ position: "relative", width: "100%", lineHeight: 0, border: "1px solid var(--line-1)", borderRadius: 6, overflow: "hidden" }}>
          <img data-testid="browser-frame" src={src} alt={`Hermes's browser showing ${s.url || "a page"}`} style={{ width: "100%", height: "auto", display: "block" }} />
          {box ? (
            <div
              data-testid="browser-highlight"
              data-state={box.pending ? "pending" : "acted"}
              style={{
                position: "absolute",
                left: box.left,
                top: box.top,
                width: box.width,
                height: box.height,
                boxSizing: "border-box",
                border: `2px solid ${box.pending ? "var(--accent)" : "var(--ok)"}`,
                borderRadius: 3,
                pointerEvents: "none",
                lineHeight: 1.2,
              }}
            >
              <span className="mono" style={{ position: "absolute", top: -18, left: -2, fontSize: 10, padding: "1px 4px", background: box.pending ? "var(--accent)" : "var(--ok)", color: "var(--srf-0)", whiteSpace: "nowrap" }}>
                {box.label}
              </span>
            </div>
          ) : null}
        </div>
      ) : active ? (
        <div style={{ ...note, padding: 24, textAlign: "center" }}>No picture yet.</div>
      ) : null}

      <div style={{ ...note, fontSize: 11, color: "var(--tx-3)" }}>
        Everything a page says is treated as untrusted. After Hermes reads a page, each click, typed entry or new address waits for your decision.
      </div>
    </div>
  );
}
