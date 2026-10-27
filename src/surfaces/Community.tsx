// =====================================================================
// citrate-core — Community (CX redesign, Pass 2) · new surface
//
// The growth surface, built 1:1 from design/CitrateCore.dc.html. It is NOT WIRED — network stats
// and the growth leaderboard need an indexer + a rewards planset before they carry real numbers, so
// this surface shows the STRUCTURE with honest-pending live data (never fabricated counts, Rule 1):
//   • Network stats + leaderboard → "—" / "lands with the indexer" (no invented figures).
//   • The 2,000-run milestone program → real program FACTS (100 / 500 / 2,000 thresholds + reward
//     tiers, settled through the ceremony) — design, not live data, so honest to show.
//   • Public link → REAL: a referral code minted in Rust at the citrate.ai join service (GROW-S1
//     task 11). It carries no group id and no wallet address.
// The whole surface carries a "pending backend · illustrative" flag.
// =====================================================================
import { useState } from "react";
import { SurfaceProps } from "./shared";
import { bridge } from "../bridge";
import type { JoinQr } from "../bridge/domains";
import { QrCode } from "../fleet/FleetWizard";
import { copyLink, YourInviteLinks } from "./InviteLinks";
import { v2InviteLinksEnabled, V2_LINKS_NOT_AVAILABLE } from "../join/v2Links";
import { createPublicLink, expiryLabel, type PublicOutcome } from "./inviteFlow";

/** The display name on a network-wide public link (there is no group behind it). */
export const NETWORK_LINK_NAME = "Open network";

const STATS: { label: string; sub: string }[] = [
  { label: "Nodes online", sub: "counted by the network indexer" },
  { label: "Groups", sub: "from signed roster assertions" },
  { label: "Members", sub: "no self-reported numbers" },
  { label: "Files co-pinned", sub: "held across group clusters" },
];

const MILESTONES: { at: string; reward: string }[] = [
  { at: "100", reward: "founding-roster SALT grant to the whole group" },
  { at: "500", reward: "growth reward + a featured slot in the directory" },
  { at: "2,000", reward: "the full 2,000-run reward, settled to every member" },
];

export function Community({ store }: SurfaceProps) {
  // GROW-S1 task 11 — a public link is a referral code from the join service (via Rust). It replaces
  // the older self-built link format.
  const [pub, setPub] = useState<PublicOutcome | null>(null);
  const [busy, setBusy] = useState(false);
  const [qr, setQr] = useState<JoinQr | null>(null);
  // Bumped after a mint so "Your invite links" (recovered from the service) reloads.
  const [linksVersion, setLinksVersion] = useState(0);
  // core#423: v2 invite links are hidden in 0.5.2 (a build-time switch).
  const v2Links = v2InviteLinksEnabled();
  const mint = async () => {
    if (busy) return;
    setBusy(true);
    try {
      const out = await createPublicLink({ invites: bridge.invites, join: bridge.join }, NETWORK_LINK_NAME, "general");
      setPub(out);
      setLinksVersion((n) => n + 1);
      if (out.kind === "link" && out.minted.shareUrl) {
        bridge.join.shareQr(out.minted.shareUrl).then(setQr).catch(() => setQr(null));
      }
    } finally {
      setBusy(false);
    }
  };

  return (
    <div style={{ padding: "20px 26px 24px", display: "flex", flexDirection: "column", gap: 16, maxWidth: 960 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 14 }}>
        <span style={{ fontFamily: "var(--font-display)", fontWeight: 420, fontSize: 24 }}>Community</span>
        <span className="mono" style={{ fontSize: 9, letterSpacing: ".1em", textTransform: "uppercase", padding: "2px 8px", borderRadius: 999, border: "1px solid var(--warn)", color: "var(--warn)", background: "var(--warn-bg)" }}>
          pending backend · illustrative
        </span>
      </div>

      {/* network stats — honest pending, no fabricated numbers */}
      <div style={{ display: "grid", gridTemplateColumns: "repeat(4,1fr)", gap: 1, background: "var(--line-1)", border: "1px solid var(--line-1)", borderRadius: "var(--r-2)", overflow: "hidden" }}>
        {STATS.map((cs) => (
          <div key={cs.label} style={{ background: "var(--srf-1)", padding: "12px 14px", display: "flex", flexDirection: "column", gap: 4 }}>
            <span className="mono" style={{ fontSize: 9.5, letterSpacing: ".13em", textTransform: "uppercase", color: "var(--tx-3)" }}>{cs.label}</span>
            <span className="tabular" style={{ fontFamily: "var(--font-display)", fontWeight: 420, fontSize: 21, color: "var(--tx-3)" }}>—</span>
            <span className="mono" style={{ fontSize: 10, color: "var(--tx-3)" }}>{cs.sub}</span>
          </div>
        ))}
      </div>

      <div style={{ display: "grid", gridTemplateColumns: "minmax(0,1fr) 320px", gap: 16, alignItems: "start" }}>
        {/* leaderboard — honest empty */}
        <div className="surface" style={{ display: "flex", flexDirection: "column" }}>
          <div style={{ display: "flex", alignItems: "center", padding: "12px 16px", borderBottom: "1px solid var(--line-1)" }}>
            <span style={{ fontSize: 13.5, fontWeight: 500 }}>Growth leaderboard</span>
            <span className="mono" style={{ marginLeft: "auto", fontSize: 9.5, letterSpacing: ".1em", textTransform: "uppercase", color: "var(--tx-3)" }}>this epoch</span>
          </div>
          <p style={{ fontSize: 12.5, lineHeight: 1.6, color: "var(--tx-3)", margin: 0, padding: 16 }}>
            Growth standings land with the network indexer. Rankings will be counted from signed roster
            assertions — never self-reported numbers — so what you see here is always real.
          </p>
        </div>

        {/* program + referral */}
        <div style={{ display: "flex", flexDirection: "column", gap: 16 }}>
          <div className="surface" style={{ padding: "16px 18px", display: "flex", flexDirection: "column", gap: 12 }}>
            <span className="eyebrow">The 2,000 run</span>
            <p style={{ fontSize: 12.5, lineHeight: 1.6, color: "var(--tx-2)", margin: 0 }}>
              Grow a group and the network rewards the whole roster at each milestone. Milestones settle
              each epoch through your ceremony — nothing is paid without your signature.
            </p>
            <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
              {MILESTONES.map((cm) => (
                <div key={cm.at} style={{ display: "flex", alignItems: "flex-start", gap: 10 }}>
                  <span className="mono tabular" style={{ width: 44, flexShrink: 0, fontSize: 12, fontWeight: 600, color: "var(--accent-text)", paddingTop: 1 }}>{cm.at}</span>
                  <span style={{ flex: 1, fontSize: 11.5, lineHeight: 1.5, color: "var(--tx-2)" }}>{cm.reward}</span>
                </div>
              ))}
            </div>
            <span className="mono" style={{ fontSize: 9.5, color: "var(--tx-3)", lineHeight: 1.6 }}>
              your group's standing appears here once the indexer lands — no progress bar is drawn from numbers we don't have yet
            </span>
          </div>

          <div className="surface" style={{ padding: "16px 18px", display: "flex", flexDirection: "column", gap: 10 }}>
            <span className="eyebrow">Your public link</span>
            {!v2Links ? (
              <span className="mono" data-testid="v2-links-not-available" style={{ fontSize: 10.5, lineHeight: 1.6, color: "var(--tx-3)" }}>
                {V2_LINKS_NOT_AVAILABLE}
              </span>
            ) : pub?.kind === "link" && pub.minted.shareUrl ? (
              <>
                <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
                  <span className="mono" style={{ flex: 1, minWidth: 0, fontSize: 11, color: "var(--tx-2)", border: "1px solid var(--line-1)", borderRadius: "var(--r-1)", padding: "8px 10px", background: "var(--srf-1)", whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>{pub.minted.shareUrl}</span>
                  <button className="btn btn-secondary btn-sm" onClick={() => pub.minted.shareUrl && copyLink(store, pub.minted.shareUrl)}>Copy</button>
                </div>
                {qr && <QrCode qr={qr} px={3} testId="community-public-qr" label="Public link QR code" />}
                <span className="mono" style={{ fontSize: 10, color: "var(--tx-3)" }}>{expiryLabel(pub.minted.exp)}</span>
              </>
            ) : (
              <>
                <button className="btn btn-secondary btn-sm" onClick={() => void mint()} disabled={busy} style={{ alignSelf: "flex-start" }}>
                  {busy ? "Creating…" : "Create a public link"}
                </button>
                {pub?.kind === "error" && <span role="alert" style={{ fontSize: 11.5, color: "var(--danger)", lineHeight: 1.5 }}>{pub.message}</span>}
              </>
            )}
            {v2Links && (
              <p className="mono" style={{ fontSize: 10, color: "var(--tx-3)", margin: 0, lineHeight: 1.6 }}>
                a short link that shows your name and points people to Citrate · no group id or wallet address in it · to add someone to a group, send a one-time link from Groups
              </p>
            )}
            <YourInviteLinks store={store} version={linksVersion} />
          </div>
        </div>
      </div>
    </div>
  );
}
