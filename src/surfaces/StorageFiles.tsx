// =====================================================================
// citrate-core — Files (CX-S2.3 + Pass-2 polish, lane s2)
//
// A drag-drop file store on IPFS over the S2.1 kubo seam: drop files to add them, pin them to
// your node (ceremony-gated SALT bond), retrieve by CID, unpin. Real data via bridge.storage
// through the storage slice. Honest states throughout (Rule 1): empty when nothing, and — when the
// kubo daemon is unreachable — an honest "your file store isn't reachable · Retry" card instead of
// a raw 500/transport dump. Add/pin/retrieve/unpin are disabled while it's down; your files stay
// safe on disk.
// =====================================================================
import { useEffect, useState } from "react";
import { SurfaceProps } from "./shared";
import { bridge } from "../bridge";
import {
  storageSlice,
  refreshPins,
  addFile,
  localPin,
  unpinCid,
  retrieveCid,
  LOCAL_PIN_MARKER,
} from "../shell/slices/storage";
import type { PinRow } from "../bridge/domains";

function humanBytes(n: number): string {
  if (!n || n < 0) return "—";
  const gb = n / 1e9;
  if (gb >= 1) return `${gb.toFixed(gb >= 10 ? 0 : 1)} GB`;
  const mb = n / 1e6;
  if (mb >= 1) return `${Math.round(mb)} MB`;
  return `${Math.max(1, Math.round(n / 1e3))} KB`;
}

function shortCid(cid: string): string {
  return cid.length > 22 ? `${cid.slice(0, 12)}…${cid.slice(-6)}` : cid;
}

const STATE_COLOR: Record<PinRow["pinState"], string> = {
  pinned: "var(--ok)",
  pinning: "var(--tx-3)",
  challenged: "var(--danger)",
  unpinned: "var(--tx-3)",
};

export function StorageFiles({ store }: SurfaceProps) {
  const st = storageSlice.use();
  const [dragging, setDragging] = useState(false);
  const down = st.kuboDown;

  useEffect(() => {
    void refreshPins();
  }, []);

  // Native Tauri file drag-drop (real filesystem paths). Sim/web has no webview drag-drop.
  useEffect(() => {
    if (bridge.mode !== "tauri") return;
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    void (async () => {
      try {
        const { getCurrentWebview } = await import("@tauri-apps/api/webview");
        const un = await getCurrentWebview().onDragDropEvent((event) => {
          const t = event.payload.type;
          setDragging(t === "over" || t === "enter");
          if (t === "drop" && !storageSlice.get().kuboDown) {
            for (const path of event.payload.paths) void addFile(path);
          }
        });
        if (cancelled) un();
        else unlisten = un;
      } catch {
        /* drag-drop unavailable — list actions still work */
      }
    })();
    return () => {
      cancelled = true;
      if (unlisten) unlisten();
    };
  }, []);

  const browse = async () => {
    if (down) return;
    if (bridge.mode !== "tauri") {
      store.toast("Adding files needs the desktop app.");
      return;
    }
    // #65 — open the native macOS file picker via the Tauri dialog plugin (now installed +
    // allowlisted `dialog:default`). Drag-and-drop still works as the alternative.
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const picked = await open({ multiple: true });
      const paths = Array.isArray(picked) ? picked : picked ? [picked] : [];
      for (const p of paths) void addFile(p);
    } catch (e) {
      store.toast("Couldn't open the file picker — drag files onto the box instead. (" + (e instanceof Error ? e.message : String(e)) + ")");
    }
  };

  return (
    <div style={{ padding: "20px 26px 24px", display: "flex", flexDirection: "column", gap: 16, maxWidth: 960 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 14 }}>
        <span style={{ fontFamily: "var(--font-display)", fontWeight: 420, fontSize: 24 }}>Files</span>
        <span className="mono" style={{ marginLeft: "auto", fontSize: 10, letterSpacing: ".06em", display: "inline-flex", alignItems: "center", gap: 6, color: "var(--tx-3)" }}>
          <span style={{ width: 6, height: 6, borderRadius: 999, background: down ? "var(--danger)" : "var(--ok)" }}></span>
          {down ? "file store unreachable" : "on IPFS · pinned to your node"}
        </span>
      </div>

      {/* honest backend-outage card (never a raw 500) */}
      {down && (
        <div className="surface" role="alert" style={{ padding: 18, display: "flex", alignItems: "center", gap: 14, borderColor: "var(--danger)" }}>
          <span style={{ flex: 1 }}>
            <span style={{ display: "block", fontSize: 13.5, fontWeight: 500, color: "var(--danger)" }}>Your file store isn't reachable right now.</span>
            <span style={{ display: "block", fontSize: 12.5, color: "var(--tx-2)", marginTop: 3, lineHeight: 1.6 }}>
              The local IPFS daemon didn't answer. Your files are safe on disk — nothing can be added or retrieved until it's back. This usually resolves with a retry.
            </span>
            <span className="mono" style={{ display: "block", fontSize: 10, color: "var(--tx-3)", marginTop: 6 }}>{st.error || "kubo · 127.0.0.1:5001 · connection refused"}</span>
          </span>
          <button className="btn btn-primary btn-sm" onClick={() => void refreshPins()}>Retry connection</button>
        </div>
      )}

      {/* drop zone */}
      <div
        style={{
          border: "1.5px dashed " + (dragging && !down ? "var(--accent)" : "var(--line-2)"),
          background: dragging && !down ? "var(--accent-wash)" : "transparent",
          borderRadius: "var(--r-2)",
          padding: "26px 20px",
          display: "flex",
          flexDirection: "column",
          alignItems: "center",
          gap: 10,
          textAlign: "center",
          opacity: down ? 0.55 : 1,
          transition: "border-color var(--dur-fast) var(--ease-standard), background var(--dur-fast) var(--ease-standard)",
        }}
      >
        <svg viewBox="0 0 24 24" width="22" height="22" fill="none" stroke="var(--tx-3)" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
          <path d="M12 16 V4 M7 9 L12 4 L17 9"></path>
          <path d="M4 16 V19 A1 1 0 0 0 5 20 H19 A1 1 0 0 0 20 19 V16"></path>
        </svg>
        <span style={{ fontSize: 13.5, fontWeight: 500 }}>{st.adding ? "Adding…" : dragging && !down ? "Drop to add" : "Drop files here"}</span>
        <span style={{ display: "flex", alignItems: "center", gap: 8 }}>
          <span style={{ fontSize: 12, color: "var(--tx-3)" }}>or</span>
          <button className="btn btn-secondary btn-sm" onClick={() => void browse()} disabled={down}>Browse…</button>
        </span>
        <span className="mono" style={{ fontSize: 10, color: "var(--tx-3)", lineHeight: 1.6, maxWidth: 420 }}>
          each file gets a CID in your local IPFS store — nothing leaves this machine until you pin or share it
        </span>
      </div>

      {/* the file store */}
      <div className="surface" style={{ display: "flex", flexDirection: "column" }}>
        <div style={{ display: "flex", alignItems: "center", padding: "12px 16px", borderBottom: "1px solid var(--line-1)" }}>
          <span style={{ fontSize: 13.5, fontWeight: 500 }}>Your store</span>
          <span className="mono" style={{ marginLeft: "auto", fontSize: 9.5, letterSpacing: ".1em", textTransform: "uppercase", color: "var(--tx-3)" }}>
            {st.pins.length} file{st.pins.length === 1 ? "" : "s"}
          </span>
        </div>
        {st.pins.length === 0 ? (
          <p style={{ fontSize: 12.5, lineHeight: 1.6, color: "var(--tx-3)", margin: 0, padding: 16 }}>
            {down ? "Can't list your files while the store is unreachable — retry above." : "No files yet. Add one above — it returns a CID you can pin, share with a group's cluster, or retrieve anywhere."}
          </p>
        ) : (
          st.pins.map((p) => {
            const busy = st.busyCid === p.cid;
            const isPinned = p.pinState === "pinned";
            return (
              <div key={p.cid} style={{ display: "flex", alignItems: "center", gap: 12, padding: "11px 16px", borderTop: "1px solid var(--line-1)" }}>
                <span style={{ flex: 1, minWidth: 0 }}>
                  <span className="mono" style={{ display: "block", fontSize: 12.5, fontWeight: 500, whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>{shortCid(p.cid)}</span>
                  <span className="mono" style={{ display: "block", fontSize: 10, color: "var(--tx-3)", marginTop: 1 }}>
                    <span style={{ color: STATE_COLOR[p.pinState] }}>{p.pinState}</span>
                    {" · "}
                    {humanBytes(p.sizeBytes)}
                    {p.bondSalt === LOCAL_PIN_MARKER ? " · local" : p.bondSalt ? " · bonded" : ""}
                  </span>
                </span>
                {p.bondSalt && p.bondSalt !== LOCAL_PIN_MARKER && (
                  <span className="mono" style={{ fontSize: 9.5, letterSpacing: ".06em", padding: "2px 8px", borderRadius: 999, border: "1px solid var(--ok)", color: "var(--ok)", background: "var(--ok-bg)" }}>bonded</span>
                )}
                <button className="btn btn-ghost btn-sm" onClick={() => { void retrieveCid(p.cid); store.toast("Retrieving to your files…"); }} disabled={busy || down}>Retrieve</button>
                {isPinned ? (
                  <button className="btn btn-ghost btn-sm" onClick={() => void unpinCid(p.cid)} disabled={busy || down} style={{ color: "var(--tx-3)" }}>{busy ? "…" : "Remove"}</button>
                ) : (
                  <button className="btn btn-ghost btn-sm" onClick={() => { void localPin(p.cid); store.toast("Bond submitted — approve the transaction in the signing screen."); }} disabled={busy || down}>{busy ? "…" : "Pin · bond"}</button>
                )}
              </div>
            );
          })
        )}
      </div>

      <p style={{ fontSize: 11, color: "var(--tx-3)", margin: 0, lineHeight: 1.6 }}>
        Pinning keeps a file on <em>your</em> node and places a staked SALT bond you approve, so the network rewards pinners for keeping your data available. You approve the bond transaction in the Signature Ceremony; nothing is signed for you. Unpinning a bonded file forfeits the remaining bond — you're told before it happens.
      </p>

      {st.lastRetrievedPath && (
        <div className="mono" style={{ fontSize: 10, color: "var(--tx-3)", lineHeight: 1.5 }}>Saved to {st.lastRetrievedPath}</div>
      )}
    </div>
  );
}
