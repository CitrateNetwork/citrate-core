// =====================================================================
// citrate-core — Settings › Network on the network profile (K6-3, K6e)
//
// Shows the network this build targets: its name, chain id, node network, RPC, explorer and
// bootnodes, every value read from the active network profile (src/network/profile.ts →
// src-tauri/network/profiles.json). Nothing here is a copy of a network value. A value the
// profile has not set reads "not set" with what sets it, never a placeholder.
//
// The state line uses the network gate's own decision (networkCompat.ts): "Update required" for
// both a retired network (the live block 0 differs) and a profile that is not set, "checking"
// until block 0 is read, and "matches" only on a definite match.
// =====================================================================
import { useEffect, useState, type ReactNode } from "react";
import { createPublicClient } from "viem";
import { citrate, readTransport } from "../chain";
import { networkCompat, type NetworkCompat } from "../shell/networkCompat";
import { gateGenesis, missingValues, profileProblems, type NetworkProfile } from "./profile";

const short = (h: string | null) => (h ? `${h.slice(0, 10)}…${h.slice(-6)}` : "unknown");

/** `host:port` of a `noise_<key>@host:port` bootnode (the whole entry if it has no `@`). */
export function bootnodeAddress(entry: string): string {
  const at = entry.indexOf("@");
  return at < 0 ? entry : entry.slice(at + 1);
}

/** The live block-0 hash for `profile`, read over its own RPC, and the gate's decision on it.
 *  No read is made for a profile that is not ready (the decision is "unset" at once), nor while
 *  `enabled` is false (the panel is not shown). */
export function useNetworkState(profile: NetworkProfile | null, enabled = true): { status: NetworkCompat; live: string | null } {
  const expected = gateGenesis(profile);
  const rpc = enabled ? (profile?.rpcUrl ?? null) : null;
  const [live, setLive] = useState<{ rpc: string | null; hash: string | null }>({ rpc: null, hash: null });
  useEffect(() => {
    if (expected === null || rpc === null) return;
    let on = true;
    createPublicClient({ chain: citrate, transport: readTransport(rpc) })
      .getBlock({ blockNumber: 0n })
      .then((b) => {
        if (on) setLive({ rpc, hash: b.hash ?? null });
      })
      .catch(() => {
        // An unread block 0 stays unknown; it is never "matches" or "retired".
        if (on) setLive({ rpc, hash: null });
      });
    return () => {
      on = false;
    };
  }, [expected, rpc]);
  const hash = live.rpc === rpc ? live.hash : null;
  return { status: networkCompat(expected, hash), live: hash };
}

const box = {
  fontSize: 11,
  background: "var(--srf-inset)",
  border: "1px solid var(--line-1)",
  borderRadius: "var(--r-1)",
  padding: "8px 10px",
  lineHeight: 1.6,
} as const;

function NotSet({ reason }: { reason: string }) {
  return (
    <span className="mono" style={{ ...box, color: "var(--tx-3)" }}>
      not set · {reason}
    </span>
  );
}

function Row({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
      <span className="lbl">{label}</span>
      {children}
    </div>
  );
}

/** The state line: the gate's decision in words. */
function NetworkState({ profile, status, live }: { profile: NetworkProfile | null; status: NetworkCompat; live: string | null }) {
  if (status === "retired" || status === "unset") {
    const problems = status === "unset" ? profileProblems(profile) : [];
    return (
      <div
        role="status"
        style={{ border: "1px solid var(--bd-1)", borderLeft: "3px solid var(--danger)", borderRadius: "var(--r-1)", padding: "10px 12px", display: "flex", flexDirection: "column", gap: 6 }}
      >
        <span style={{ fontSize: 13, fontWeight: 500 }}>Update required</span>
        {status === "retired" ? (
          <span style={{ fontSize: 12, color: "var(--tx-2)", lineHeight: 1.5 }}>
            The live network has a different genesis from the one this build targets, so this version does not sync or sign. Update Citrate Core.
            <span className="mono" style={{ display: "block", fontSize: 10.5, color: "var(--tx-3)" }}>
              this build: {short(gateGenesis(profile))} · live network: {short(live)}
            </span>
          </span>
        ) : (
          <span style={{ fontSize: 12, color: "var(--tx-2)", lineHeight: 1.5 }}>
            This build has no network set, so it does not sync or sign anything. Update Citrate Core.
            {problems.length > 0 && (
              <span className="mono" style={{ display: "block", fontSize: 10.5, color: "var(--tx-3)" }}>
                {problems.join(" · ")}
              </span>
            )}
          </span>
        )}
      </div>
    );
  }
  const words = status === "compatible" ? "the live network matches this build (block 0)" : "checking the live network's block 0…";
  const color = status === "compatible" ? "var(--ok)" : "var(--tx-3)";
  return (
    <span style={{ display: "inline-flex", alignItems: "center", gap: 8 }}>
      <span style={{ width: 7, height: 7, borderRadius: 999, background: color }}></span>
      <span className="mono" style={{ fontSize: 11, color: "var(--tx-2)" }}>
        {words}
      </span>
    </span>
  );
}

/** Settings › Network: every value from `profile`; `status`/`live` from the network gate. */
export function NetworkProfilePanel({ profile, status, live }: { profile: NetworkProfile | null; status: NetworkCompat; live: string | null }) {
  const reason = (field: string) => (profile ? (missingValues(profile).find((m) => m.field === field)?.reason ?? "") : "no active network profile");
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 14 }}>
      <Row label="Network this build targets">
        <span style={{ fontSize: 13 }}>
          {profile ? profile.label : "no active network profile"}
          {profile && (
            <span className="mono" style={{ fontSize: 11, color: "var(--tx-3)", marginLeft: 8 }}>
              chain {profile.chainId ?? "not set"} · node network {profile.nodeNetwork ?? "not set"}
            </span>
          )}
        </span>
        <NetworkState profile={profile} status={status} live={live} />
      </Row>
      <Row label="RPC">
        {profile?.rpcUrl ? (
          <span className="mono" style={box}>
            {profile.rpcUrl}
          </span>
        ) : (
          <NotSet reason={reason("rpcUrl")} />
        )}
      </Row>
      <Row label="Explorer">
        {profile?.explorerUrl ? (
          <span className="mono" style={box}>
            {profile.explorerUrl}
          </span>
        ) : (
          <NotSet reason={reason("explorerUrl")} />
        )}
      </Row>
      <Row label="Bootnodes">
        {profile?.bootnodes && profile.bootnodes.length > 0 ? (
          <span className="mono" style={box}>
            {profile.bootnodes.map((b, i) => (
              <span key={b} title={b}>
                {i > 0 && " · "}
                {bootnodeAddress(b)}
              </span>
            ))}
          </span>
        ) : (
          <NotSet reason={reason("bootnodes")} />
        )}
      </Row>
    </div>
  );
}
