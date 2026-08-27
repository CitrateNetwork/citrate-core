// =====================================================================
// citrate-core — Files (CX-S2.3, lane s2)
//
// A drag-drop file store on IPFS over the S2.1 kubo seam: drop files to add them, pin them to
// your node, retrieve by CID, unpin. Real data via bridge.storage folded through the storage
// slice. Honest states throughout (Rule 1): empty when nothing, real error text on failure.
//
// S2.3 pins LOCALLY (to this node's kubo). The network-wide ceremony-gated SALT bond (paying the
// network to keep your file) is S2.2 — blocked on a chain-side fix
// (docs/FINDING_PIN_COMMD_BOND_2026-08-26.md) — so the surface says so plainly rather than
// implying a network bond it can't yet place.
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
  return cid.length > 18 ? `${cid.slice(0, 10)}…${cid.slice(-6)}` : cid;
}

const STATE_COLOR: Record<PinRow["pinState"], string> = {
  pinned: "var(--ok, #2e9e5b)",
  pinning: "var(--tx-3)",
  challenged: "var(--bad, #c0392b)",
  unpinned: "var(--tx-3)",
};

export function StorageFiles({ store }: SurfaceProps) {
  const st = storageSlice.use();
  const [dragging, setDragging] = useState(false);

  useEffect(() => {
    void refreshPins();
  }, []);

  // Native Tauri file drag-drop (gives real filesystem paths; no plugin needed). Sim/web builds
  // have no webview drag-drop, so this only arms in tauri mode.
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
          if (t === "drop") {
            for (const path of event.payload.paths) void addFile(path);
          }
        });
        if (cancelled) un();
        else unlisten = un;
      } catch {
        /* drag-drop unavailable — the surface still works via the list actions */
      }
    })();
    return () => {
      cancelled = true;
      if (unlisten) unlisten();
    };
  }, []);

  return (
    <div style={{ padding: "20px 26px 24px", display: "flex", flexDirection: "column", gap: 16, maxWidth: 820 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 18 }}>
        <span style={{ fontFamily: "var(--font-display)", fontWeight: 420, fontSize: 24 }}>Files</span>
        <span style={{ marginLeft: "auto", fontSize: 11, color: "var(--tx-3)" }}>
          stored on IPFS, pinned to your node
        </span>
      </div>

      {st.error && (
        <div
          className="surface"
          role="alert"
          style={{ padding: "12px 16px", fontSize: 12.5, color: "var(--bad, #c0392b)", lineHeight: 1.5 }}
        >
          {st.error}
        </div>
      )}

      {/* ---- drop zone ---- */}
      <div
        className="surface"
        style={{
          padding: "28px 18px",
          textAlign: "center",
          border: dragging ? "2px dashed var(--ok, #2e9e5b)" : "2px dashed var(--ln, rgba(0,0,0,0.14))",
          borderRadius: 12,
          background: dragging ? "var(--bg-2, rgba(46,158,91,0.06))" : "transparent",
          transition: "border-color .12s, background .12s",
        }}
      >
        <div style={{ fontSize: 14, fontWeight: 520 }}>
          {st.adding ? "Adding…" : dragging ? "Drop to add" : "Drag files here to store them"}
        </div>
        <div style={{ fontSize: 11.5, color: "var(--tx-3)", marginTop: 6, lineHeight: 1.5 }}>
          Files are added to IPFS and get a content address (CID). Pin one to keep it on your node.
        </div>
      </div>

      {/* ---- honest network-bond note (D-22 subsidy framing, RT-4; not yet live) ---- */}
      <div style={{ fontSize: 10.5, color: "var(--tx-3)", lineHeight: 1.55 }}>
        Pinning keeps a file on <em>your</em> node. Network storage — where the network rewards
        pinners for keeping your data available, backed by a staked SALT bond from a shared
        subsidy pool — arrives once the on-chain bond is finalized. Until then, files pin locally.
      </div>

      {/* ---- the file store ---- */}
      <div className="surface" style={{ display: "flex", flexDirection: "column" }}>
        {st.pins.length === 0 ? (
          <div style={{ padding: "18px", fontSize: 12.5, color: "var(--tx-3)", lineHeight: 1.6 }}>
            No files yet. Drag one onto the box above to store it.
          </div>
        ) : (
          st.pins.map((p) => {
            const busy = st.busyCid === p.cid;
            const isPinned = p.pinState === "pinned";
            return (
              <div
                key={p.cid}
                style={{
                  display: "flex",
                  alignItems: "center",
                  gap: 12,
                  padding: "12px 16px",
                  borderTop: "1px solid var(--ln, rgba(0,0,0,0.06))",
                }}
              >
                <div style={{ display: "flex", flexDirection: "column", gap: 2, minWidth: 0, flex: 1 }}>
                  <span className="mono" style={{ fontSize: 12, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                    {shortCid(p.cid)}
                  </span>
                  <span style={{ fontSize: 10.5, color: "var(--tx-3)" }}>
                    <span style={{ color: STATE_COLOR[p.pinState] }}>{p.pinState}</span>
                    {" · "}
                    {humanBytes(p.sizeBytes)}
                    {p.bondSalt === LOCAL_PIN_MARKER ? " · local" : p.bondSalt ? " · bonded" : ""}
                  </span>
                </div>
                <button
                  className="btn btn-ghost btn-sm"
                  onClick={() => {
                    void retrieveCid(p.cid);
                    store.toast("Retrieving to your files…");
                  }}
                  disabled={busy}
                >
                  Retrieve
                </button>
                {isPinned ? (
                  <button className="btn btn-ghost btn-sm" onClick={() => void unpinCid(p.cid)} disabled={busy}>
                    {busy ? "…" : "Unpin"}
                  </button>
                ) : (
                  <button className="btn btn-sm" onClick={() => void localPin(p.cid)} disabled={busy}>
                    {busy ? "…" : "Pin"}
                  </button>
                )}
              </div>
            );
          })
        )}
      </div>

      {st.lastRetrievedPath && (
        <div style={{ fontSize: 10.5, color: "var(--tx-3)", lineHeight: 1.5 }} className="mono">
          Saved to {st.lastRetrievedPath}
        </div>
      )}
    </div>
  );
}
