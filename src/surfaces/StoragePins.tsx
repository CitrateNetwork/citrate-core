// =====================================================================
// citrate-core — Pins (K4). One of the three storage components (Files, Memory, Pins) that
// Node › Storage composes; also routed today as the "Pins" tab under Files.
//
// Data sources (Rule 7):
// - The pin list: `storage_list` → the local pin index (`<app data>/ipfs/pins.json`) reconciled
//   against the IPFS daemon's live recursive pins (`/api/v0/pin/ls`). The daemon is the truth for
//   whether a CID is kept on this node.
// - The bond parameters: `storage_pin_params` → IPFSIncentivesV3 `BOND()`, `REWARD()`, `ROUNDS()`
//   via eth_call on chain 40204 (address from the pinned address book).
//
// UI truth contract §1/§2/§4: IPFSIncentivesV3 is deployed but dormant and unfunded, so this shows
// "Pin bonds not live yet", reads the contract as PARAMETERS only, and never shows Pinned,
// Challenged or Released as a live state. Every pin is "Local pin · no bond". A failed read shows
// "—" with its reason, never 0.
// =====================================================================
import { useEffect } from "react";
import { SurfaceProps } from "./shared";
import { storageSlice, refreshPins, refreshPinParams, localPin, unpinCid } from "../shell/slices/storage";
import { formatSaltWei, humanBytes, pinStateLabel, PIN_BONDS_NOT_LIVE, LOCAL_PIN_LABEL } from "./storageTruth";

function shortCid(cid: string): string {
  return cid.length > 22 ? `${cid.slice(0, 12)}…${cid.slice(-6)}` : cid;
}

function ParamRow({ label, value, hint }: { label: string; value: string; hint?: string }) {
  return (
    <div style={{ display: "flex", alignItems: "baseline", gap: 10 }}>
      <span style={{ fontSize: 12.5, flex: 1 }}>{label}</span>
      <span className="mono tabular" style={{ fontSize: 12 }} title={hint}>
        {value}
      </span>
    </div>
  );
}

/** The IPFSIncentivesV3 parameter card: parameters only, never a live pin state. */
export function PinBondParams() {
  const st = storageSlice.use();
  const p = st.pinParams;
  const rounds = p?.rounds == null ? "—" : String(p.rounds);
  const reward = p?.rewardWei == null ? "—" : `${formatSaltWei(p.rewardWei)} over ${rounds} rounds`;
  return (
    <div className="surface" style={{ padding: 16, display: "flex", flexDirection: "column", gap: 10 }} data-testid="pin-bond-params">
      <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
        <span className="eyebrow">Pin bonds</span>
        <span
          className="mono"
          style={{ marginLeft: "auto", fontSize: 9.5, letterSpacing: ".06em", padding: "2px 8px", borderRadius: 999, border: "1px solid var(--line-2)", color: "var(--tx-3)" }}
        >
          {PIN_BONDS_NOT_LIVE}
        </span>
      </div>
      <p style={{ fontSize: 12, lineHeight: 1.55, color: "var(--tx-2)", margin: 0 }}>
        The pin-bond contract (IPFSIncentivesV3) is deployed but not funded, so no pin carries a bond yet and nothing
        is challenged or released on chain. Bonds go live with the genesis contract. Until then the values below are
        the contract&apos;s parameters, not anything you hold.
      </p>
      <ParamRow label="Bond per pin (parameter)" value={formatSaltWei(p?.bondWei)} />
      <ParamRow label="Reward per pin (parameter)" value={reward} />
      {p && (
        <span className="mono" style={{ fontSize: 10, color: "var(--tx-3)", wordBreak: "break-all" }}>
          source · {p.source} · {p.contract}
        </span>
      )}
      {st.pinParamsError && (
        <span className="mono" style={{ fontSize: 10, color: "var(--warn, var(--tx-3))" }}>
          parameters unavailable: {st.pinParamsError}
        </span>
      )}
      {p && p.errors.length > 0 && (
        <span className="mono" style={{ fontSize: 10, color: "var(--warn, var(--tx-3))" }}>
          {p.errors.join(" · ")}
        </span>
      )}
    </div>
  );
}

export function StoragePins(_props: SurfaceProps) {
  const st = storageSlice.use();
  const down = st.kuboDown;

  useEffect(() => {
    void refreshPins();
    void refreshPinParams();
  }, []);

  const pinned = st.pins.filter((p) => p.pinState === "pinned");

  return (
    <div style={{ padding: "20px 26px 24px", display: "flex", flexDirection: "column", gap: 16, maxWidth: 960 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 14 }}>
        <span style={{ fontFamily: "var(--font-display)", fontWeight: 420, fontSize: 24 }}>Pins</span>
        <span className="mono" style={{ marginLeft: "auto", fontSize: 10, color: "var(--tx-3)" }}>
          {down ? "file store unreachable" : `${pinned.length} kept on this node`}
        </span>
      </div>

      <PinBondParams />

      <div className="surface" style={{ display: "flex", flexDirection: "column" }}>
        <div style={{ display: "flex", alignItems: "center", padding: "12px 16px", borderBottom: "1px solid var(--line-1)" }}>
          <span style={{ fontSize: 13.5, fontWeight: 500 }}>Pin index</span>
          <span className="mono" style={{ marginLeft: "auto", fontSize: 9.5, letterSpacing: ".1em", textTransform: "uppercase", color: "var(--tx-3)" }}>
            local · this node
          </span>
        </div>
        {down ? (
          <div role="alert" style={{ padding: 16, display: "flex", alignItems: "center", gap: 12 }}>
            <span style={{ flex: 1, fontSize: 12.5, color: "var(--tx-2)" }}>
              The local IPFS store didn&apos;t answer, so the pin index can&apos;t be read right now.
              <span className="mono" style={{ display: "block", fontSize: 10, color: "var(--tx-3)", marginTop: 4 }}>{st.error}</span>
            </span>
            <button className="btn btn-secondary btn-sm" onClick={() => void refreshPins()}>
              Retry
            </button>
          </div>
        ) : st.pins.length === 0 ? (
          <p style={{ fontSize: 12.5, lineHeight: 1.6, color: "var(--tx-3)", margin: 0, padding: 16 }}>
            No pins yet. Files you add are kept on this node as a {LOCAL_PIN_LABEL.toLowerCase()}.
          </p>
        ) : (
          st.pins.map((p) => {
            const busy = st.busyCid === p.cid;
            const isPinned = p.pinState === "pinned";
            return (
              <div key={p.cid} style={{ display: "flex", alignItems: "center", gap: 12, padding: "11px 16px", borderTop: "1px solid var(--line-1)" }}>
                <span style={{ flex: 1, minWidth: 0 }}>
                  <span className="mono" style={{ display: "block", fontSize: 12.5, fontWeight: 500, whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }} title={p.cid}>
                    {shortCid(p.cid)}
                  </span>
                  <span className="mono" style={{ display: "block", fontSize: 10, color: "var(--tx-3)", marginTop: 1 }}>
                    <span style={{ color: isPinned ? "var(--ok)" : "var(--tx-3)" }}>{pinStateLabel(p)}</span>
                    {" · "}
                    {humanBytes(p.sizeBytes)}
                  </span>
                </span>
                {isPinned ? (
                  <button className="btn btn-ghost btn-sm" onClick={() => void unpinCid(p.cid)} disabled={busy} style={{ color: "var(--tx-3)" }}>
                    {busy ? "…" : "Remove pin"}
                  </button>
                ) : (
                  <button className="btn btn-ghost btn-sm" onClick={() => void localPin(p.cid)} disabled={busy}>
                    {busy ? "…" : "Pin locally"}
                  </button>
                )}
              </div>
            );
          })
        )}
      </div>

      <p style={{ fontSize: 11, color: "var(--tx-3)", margin: 0, lineHeight: 1.6 }}>
        source · the local pin index reconciled with the IPFS daemon&apos;s pins (127.0.0.1:5001). A local pin keeps the
        file on this node only; it places no bond and sends no transaction.
      </p>
    </div>
  );
}
