// =====================================================================
// citrate-core: Node › Advanced "Old network data" (CORE-N3 follow-up, 0.5.3)
//
// A genesis switch keeps the previous network's chain data in the node data dir
// (`chain.<profile>.bak`, node_genesis.rs). The node never reads it again; this panel shows how big
// it is and removes it after one confirmation.
//
// Data sources (Rule 7): the node data dir, through node_old_network_data (list, sizes) and
// node_remove_old_network_data (one folder, at the size shown). Rust refuses anything that is not a
// chain backup in the node data dir, holds anything but chain files, or changed since it was shown
// (chain_backup.rs); the wallet, keys and current chain can't be reached through it.
// =====================================================================
import { useCallback, useEffect, useState } from "react";
import { bridge } from "../../bridge";
import type { OldNetworkDataView } from "../../bridge/domains";
import { Panel, StateCard, errText } from "./parts";

export interface OldNetworkDataReads {
  list(): Promise<OldNetworkDataView[]>;
  remove(name: string, confirmedBytes: number): Promise<number>;
}

export const LIVE_OLD_NETWORK_DATA: OldNetworkDataReads = {
  list: () => bridge.node.oldNetworkData(),
  remove: (name, bytes) => bridge.node.removeOldNetworkData(name, bytes),
};

/** Decimal units, one decimal above a kilobyte ("41.2 GB"). */
export function formatBytes(n: number): string {
  if (n < 1000) return `${n} B`;
  const units = ["kB", "MB", "GB", "TB"];
  let v = n;
  let u = -1;
  while (v >= 1000 && u < units.length - 1) {
    v /= 1000;
    u++;
  }
  return `${v.toFixed(1)} ${units[u]}`;
}

type Read = { state: "reading" } | { state: "ok"; value: OldNetworkDataView[] } | { state: "error"; error: string };

export function OldNetworkData({ reads, mode }: { reads: OldNetworkDataReads; mode: "tauri" | "sim" }) {
  const [list, setList] = useState<Read>({ state: "reading" });
  const [asking, setAsking] = useState<OldNetworkDataView | null>(null);
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<{ ok: boolean; text: string } | null>(null);

  const load = useCallback(() => {
    reads
      .list()
      .then((value) => setList({ state: "ok", value }))
      .catch((e) => setList({ state: "error", error: errText(e) }));
  }, [reads]);

  useEffect(() => {
    if (mode === "tauri") load();
  }, [mode, load]);

  const confirm = async () => {
    if (!asking || busy) return;
    setBusy(true);
    try {
      const freed = await reads.remove(asking.name, asking.bytes);
      setResult({ ok: true, text: `Removed ${asking.name} (${formatBytes(freed)} freed).` });
      setAsking(null);
      load();
    } catch (e) {
      setResult({ ok: false, text: errText(e) });
      setAsking(null);
    } finally {
      setBusy(false);
    }
  };

  if (mode !== "tauri") return null;
  return (
    <Panel title="Old network data" caption="node_old_network_data (the node folder)" testid="old-network-data">
      <div style={{ padding: "12px 18px", display: "flex", flexDirection: "column", gap: 10 }}>
        {list.state === "reading" && <span style={{ fontSize: 12.5, color: "var(--tx-3)" }}>Reading…</span>}
        {list.state === "error" && (
          <StateCard kind="error" title="Couldn't read the node folder" body="Whether old network data is kept here is unknown." why={"node_old_network_data → error: " + list.error} cta={{ label: "Try again", go: load }} />
        )}
        {list.state === "ok" && list.value.length === 0 && (
          <span style={{ fontSize: 12.5, color: "var(--tx-2)" }}>No old network data in the node folder.</span>
        )}
        {list.state === "ok" &&
          list.value.map((b) => (
            <div key={b.name} style={{ display: "flex", alignItems: "baseline", gap: 12 }}>
              <span className="mono" style={{ fontSize: 12, flex: 1, overflowWrap: "anywhere" }}>
                {b.name}
              </span>
              <span className="mono tabular" style={{ fontSize: 12 }}>
                {formatBytes(b.bytes)}
              </span>
              <button type="button" className="btn" data-testid={`old-data-remove-${b.name}`} disabled={busy} onClick={() => setAsking(b)}>
                Remove
              </button>
            </div>
          ))}
        {asking && (
          <div role="alertdialog" aria-label="Remove old network data" style={{ display: "flex", flexDirection: "column", gap: 8, padding: 12, border: "1px solid var(--warn)", borderRadius: "var(--r-1)" }}>
            <span style={{ fontSize: 12.5, lineHeight: 1.6 }}>
              Remove {asking.name} ({formatBytes(asking.bytes)})? This is the previous network's chain data; this node no longer reads it. Your wallet, keys and the current chain are not touched. It can't be undone.
            </span>
            <div style={{ display: "flex", gap: 8 }}>
              <button type="button" className="btn" data-testid="old-data-confirm" disabled={busy} onClick={() => void confirm()}>
                Remove {formatBytes(asking.bytes)}
              </button>
              <button type="button" className="btn" data-testid="old-data-cancel" disabled={busy} onClick={() => setAsking(null)}>
                Cancel
              </button>
            </div>
          </div>
        )}
        {result && (
          <span role="status" style={{ fontSize: 12.5, color: result.ok ? "var(--tx-2)" : "var(--danger)", overflowWrap: "anywhere" }}>
            {result.text}
          </span>
        )}
        <span className="mono" style={{ fontSize: 10.5, color: "var(--tx-3)" }}>
          source: the node folder's chain.&lt;network&gt;.bak folders, kept by a network switch (node_genesis.rs)
        </span>
      </div>
    </Panel>
  );
}
