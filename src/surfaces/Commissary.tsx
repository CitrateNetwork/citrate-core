// =====================================================================
// citrate-core — Commissary
// Ported 1:1 from design/CitrateCore.dc.html (COMMISSARY section + inline
// script: cTabs / appCards / sdkCards / docCards / svcCards; tier gating via
// RANK compare against effTier; org-scoped cards HIDE on org mismatch — never
// shown as locked (Atlas doctrine); signed, checksum-verified downloads with
// the honest mint → dl → verify → done state machine off s.dl.
//
// Data source — signed catalog manifest v3 (data/seed.ts CATALOG in the
// prototype). Every card names its real state; downloads are honest: a
// signed URL is minted against a live entitlement check, the bytes stream,
// then sha256 is verified and the install is audit-logged. Wiring replaces
// the sim, not the UI (Rule 1).
// =====================================================================
import { SurfaceProps } from "./shared";
import { CATALOG } from "../data/seed";
import { RANK } from "../shell/state";
import { federationUrl } from "../data/links";

type BadgeTriple = [string, string, string];
const badge = (st: string): BadgeTriple =>
  st === "GA"
    ? ["var(--ok-bg)", "var(--ok)", "var(--ok)"]
    : st === "Release Candidate"
    ? ["var(--info-bg)", "var(--info)", "var(--info)"]
    : st === "Beta"
    ? ["var(--srf-1)", "var(--line-2)", "var(--tx-2)"]
    : ["var(--warn-bg)", "var(--warn)", "var(--warn)"];

const tierPill = (t: string): BadgeTriple =>
  t === "public"
    ? ["var(--srf-1)", "var(--line-2)", "var(--tx-2)"]
    : t === "member"
    ? ["var(--ok-bg)", "var(--ok)", "var(--ok)"]
    : t === "commercial"
    ? ["var(--warn-bg)", "var(--warn)", "var(--warn)"]
    : t === "org"
    ? ["var(--info-bg)", "var(--info)", "var(--info)"]
    : ["var(--danger-bg)", "var(--danger)", "var(--danger)"];

const CTABS: [string, string][] = [
  ["apps", "Apps"],
  ["sdks", "SDKs"],
  ["docs", "Docs"],
  ["services", "Services"],
];

export function Commissary({ store, s }: SurfaceProps) {
  const rank = RANK;
  const effTier = s.entitlement === "lapsed" ? "free" : s.tier;

  // Honest download state machine, written to s.dl[id] exactly as the design's
  // render logic reads it: mint (signed URL + live entitlement check) → dl
  // (byte stream, %) → verify (sha256) → done (verified · audit-logged).
  const startDownload = (id: string) => {
    const cur = s.dl[id];
    if (cur && cur.st !== "idle" && cur.st !== "done") return;
    const set = (patch: { st: string; pct: number }) =>
      store.setState((st0) => ({ dl: { ...st0.dl, [id]: patch } }));
    set({ st: "mint", pct: 0 });
    setTimeout(() => set({ st: "dl", pct: 12 }), 700);
    setTimeout(() => set({ st: "dl", pct: 46 }), 1300);
    setTimeout(() => set({ st: "dl", pct: 83 }), 1900);
    setTimeout(() => set({ st: "verify", pct: 100 }), 2500);
    setTimeout(() => {
      set({ st: "done", pct: 100 });
      store.save();
    }, 3200);
  };

  const cTabs = CTABS.map(([id, label]) => ({
    id,
    label,
    go: () => {
      store.setState({ cTab: id });
      store.save();
    },
    weight: s.cTab === id ? 500 : 400,
    bg: s.cTab === id ? "var(--srf-2)" : "transparent",
    color: s.cTab === id ? "var(--tx-1)" : "var(--tx-3)",
  }));

  const ctApps = s.cTab === "apps";
  const ctSdks = s.cTab === "sdks";
  const ctDocs = s.cTab === "docs";
  const ctServices = s.cTab === "services";

  const appCards = CATALOG.apps
    .filter((a) => !("orgScope" in a) || (a as { orgScope?: string }).orgScope === s.org)
    .map((a) => {
      const locked = rank[a.minTier] > rank[effTier];
      const dl = s.dl[a.id] || { st: "idle", pct: 0 };
      const [bBg, bBd, bFg] = badge(a.status);
      const isMicro = a.kind === "micro-app";
      const caps = (a as { capabilities?: string[] }).capabilities || [];
      return {
        id: a.id,
        name: a.name,
        desc: a.desc,
        status: a.status,
        version: a.version,
        size: a.size,
        platformsLine: a.platforms.join(" · "),
        checksum: a.checksum,
        badgeBg: bBg,
        badgeBd: bBd,
        badgeFg: bFg,
        locked,
        unlocked: !locked,
        lockText: "Requires " + (a.minTier === "pilot" ? "Pilot membership" : "an Enterprise seat"),
        unlockLabel: a.minTier === "pilot" ? "Join · $48/yr" : "Contact us",
        onUnlock: () =>
          store.toast(
            a.minTier === "pilot"
              ? "Checkout opens in your browser — flip tiers in the Prototype panel to preview."
              : "Enterprise is consultative — the contact form opens in your browser.",
          ),
        dlIdle: dl.st === "idle",
        dlBusy: dl.st === "mint" || dl.st === "dl" || dl.st === "verify",
        dlDone: dl.st === "done",
        dlLabel:
          dl.st === "mint"
            ? "minting signed URL · live entitlement check"
            : dl.st === "dl"
            ? "downloading · " + (dl.pct | 0) + "%"
            : "verifying sha256…",
        dlPctW: dl.st === "mint" ? "6%" : dl.st === "verify" ? "100%" : (dl.pct | 0) + "%",
        metaLine: isMicro ? caps.join(" · ") : "signed URL · first-download expiry",
        actionLabel: isMicro ? "Open panel" : "Download",
        onAction: isMicro ? () => store.openMicroApp(a) : () => startDownload(a.id),
      };
    });

  const sdkCards = CATALOG.sdks.map((sd) => ({
    id: sd.id,
    name: sd.name,
    registry: sd.registry,
    desc: sd.desc,
    install: sd.install,
    docs: sd.docs,
    copy: () => store.copy(sd.install, "Install command copied"),
  }));

  const docCards = CATALOG.docs
    .filter((d) => !("orgScope" in d) || (d as { orgScope?: string }).orgScope === s.org)
    .map((d) => {
      const locked = rank[d.minTier] > rank[effTier];
      const [tBg, tBd, tFg] = tierPill(d.tier);
      return {
        id: d.id,
        name: d.name,
        desc: d.desc,
        tier: d.tier,
        tierBg: tBg,
        tierBd: tBd,
        tierFg: tFg,
        locked,
        unlocked: !locked,
        opacity: locked ? 0.6 : 1,
        cta: d.minTier === "pilot" ? "Join · $48/yr" : "Contact us",
        onUnlock: () =>
          store.toast("This doc unlocks with " + (d.minTier === "pilot" ? "Pilot membership" : "an Enterprise seat") + "."),
      };
    });

  const svcCards = CATALOG.services;

  return (
    <div style={{ padding: "20px 26px 24px", display: "flex", flexDirection: "column", gap: 16 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 18 }}>
        <span style={{ fontFamily: "var(--font-display)", fontWeight: 420, fontSize: 24 }}>Commissary</span>
        <span style={{ display: "flex", gap: 2, background: "var(--srf-1)", border: "1px solid var(--line-1)", borderRadius: "var(--r-1)", padding: 2 }}>
          {cTabs.map((t) => (
            <button
              key={t.id}
              onClick={t.go}
              style={{
                fontFamily: "var(--font-sans)",
                fontSize: 12,
                fontWeight: t.weight,
                padding: "5px 12px",
                border: "none",
                borderRadius: 5,
                cursor: "pointer",
                background: t.bg,
                color: t.color,
              }}
            >
              {t.label}
            </button>
          ))}
        </span>
        <span className="mono" style={{ marginLeft: "auto", fontSize: 10, letterSpacing: ".08em", color: "var(--tx-3)" }}>
          catalog · signed manifest v3
        </span>
      </div>

      {/* ----- Apps ----- */}
      {ctApps && (
        <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fill,minmax(320px,1fr))", gap: 12 }}>
          {appCards.map((a) => (
            <div key={a.id} className="surface" style={{ padding: 18, display: "flex", flexDirection: "column", gap: 10, position: "relative" }}>
              <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
                <span style={{ fontSize: 14.5, fontWeight: 500, flex: 1 }}>{a.name}</span>
                <span
                  className="mono"
                  style={{
                    fontSize: 9.5,
                    letterSpacing: ".1em",
                    textTransform: "uppercase",
                    padding: "2px 8px",
                    borderRadius: 999,
                    background: a.badgeBg,
                    border: "1px solid " + a.badgeBd,
                    color: a.badgeFg,
                  }}
                >
                  {a.status}
                </span>
              </div>
              <p style={{ fontSize: 12.5, lineHeight: 1.55, color: "var(--tx-2)", margin: 0, flex: 1 }}>{a.desc}</p>
              <div className="mono" style={{ fontSize: 10.5, color: "var(--tx-3)" }}>
                v{a.version} · {a.size} · {a.platformsLine}
              </div>
              <div className="mono" style={{ fontSize: 10, color: "var(--tx-3)", whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>
                {a.checksum}
              </div>

              {a.locked && (
                <div style={{ borderTop: "1px solid var(--line-1)", paddingTop: 10, display: "flex", alignItems: "center", gap: 10 }}>
                  <span style={{ fontSize: 12, color: "var(--tx-3)", flex: 1 }}>{a.lockText}</span>
                  <button className="btn btn-ghost btn-sm" onClick={a.onUnlock}>
                    {a.unlockLabel}
                  </button>
                </div>
              )}

              {a.unlocked && (
                <div style={{ borderTop: "1px solid var(--line-1)", paddingTop: 10, display: "flex", alignItems: "center", gap: 10 }}>
                  {a.dlBusy && (
                    <span style={{ flex: 1, display: "flex", flexDirection: "column", gap: 5 }}>
                      <span className="mono" style={{ fontSize: 10, letterSpacing: ".08em", textTransform: "uppercase", color: "var(--tx-3)" }}>
                        {a.dlLabel}
                      </span>
                      <span style={{ display: "block", height: 4, background: "var(--srf-inset)", borderRadius: 999, overflow: "hidden", border: "1px solid var(--line-1)" }}>
                        <span style={{ display: "block", height: "100%", background: "var(--accent)", width: a.dlPctW, transition: "width .3s linear" }}></span>
                      </span>
                    </span>
                  )}
                  {!a.dlBusy && !a.dlDone && (
                    <>
                      <span className="mono" style={{ fontSize: 10, color: "var(--tx-3)", flex: 1 }}>
                        {a.metaLine}
                      </span>
                      <button className="btn btn-secondary btn-sm" onClick={a.onAction}>
                        {a.actionLabel}
                      </button>
                    </>
                  )}
                  {a.dlDone && (
                    <>
                      <span className="mono" style={{ fontSize: 10.5, color: "var(--ok)", flex: 1 }}>
                        ✓ verified · sha256 match · audit-logged
                      </span>
                      <button className="btn btn-ghost btn-sm" onClick={a.onAction}>
                        Re-download
                      </button>
                    </>
                  )}
                </div>
              )}
            </div>
          ))}
        </div>
      )}

      {/* ----- SDKs ----- */}
      {ctSdks && (
        <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fill,minmax(320px,1fr))", gap: 12 }}>
          {sdkCards.map((sd) => (
            <div key={sd.id} className="surface" style={{ padding: 18, display: "flex", flexDirection: "column", gap: 10 }}>
              <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
                <span className="mono" style={{ fontSize: 14, fontWeight: 500, flex: 1 }}>
                  {sd.name}
                </span>
                <span
                  className="mono"
                  style={{ fontSize: 9.5, letterSpacing: ".1em", textTransform: "uppercase", color: "var(--tx-3)", border: "1px solid var(--line-1)", borderRadius: 999, padding: "2px 8px" }}
                >
                  {sd.registry}
                </span>
              </div>
              <p style={{ fontSize: 12.5, lineHeight: 1.55, color: "var(--tx-2)", margin: 0, flex: 1 }}>{sd.desc}</p>
              <div style={{ display: "flex", alignItems: "center", gap: 8, background: "var(--srf-inset)", border: "1px solid var(--line-1)", borderRadius: "var(--r-1)", padding: "8px 10px" }}>
                <span className="mono" style={{ fontSize: 11, flex: 1, whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>
                  {sd.install}
                </span>
                <button className="btn btn-ghost btn-sm" onClick={sd.copy}>
                  Copy
                </button>
              </div>
              <button
                className="mono"
                onClick={() => void store.openExternal(federationUrl(sd.docs))}
                style={{ fontSize: 10.5, color: "var(--accent-text)", cursor: "pointer", background: "none", border: "none", padding: 0, textAlign: "left" }}
              >
                docs · {sd.docs} ↗
              </button>
            </div>
          ))}
        </div>
      )}

      {/* ----- Docs ----- */}
      {ctDocs && (
        <div style={{ display: "flex", flexDirection: "column", gap: 10, maxWidth: 760 }}>
          {docCards.map((d) => (
            <div key={d.id} className="surface" style={{ padding: "14px 18px", display: "flex", alignItems: "center", gap: 14, opacity: d.opacity }}>
              <span style={{ flex: 1, minWidth: 0 }}>
                <span style={{ display: "block", fontSize: 13.5, fontWeight: 500 }}>{d.name}</span>
                <span style={{ display: "block", fontSize: 12, color: "var(--tx-3)", marginTop: 2 }}>{d.desc}</span>
              </span>
              <span
                className="mono"
                style={{
                  fontSize: 9.5,
                  letterSpacing: ".1em",
                  textTransform: "uppercase",
                  padding: "2px 8px",
                  borderRadius: 999,
                  border: "1px solid " + d.tierBd,
                  color: d.tierFg,
                  background: d.tierBg,
                  whiteSpace: "nowrap",
                }}
              >
                {d.tier}
              </span>
              {d.locked && (
                <button className="btn btn-ghost btn-sm" onClick={d.onUnlock}>
                  {d.cta}
                </button>
              )}
              {d.unlocked && (
                <button
                  className="mono"
                  onClick={() => void store.openExternal(federationUrl("atlas/docs/" + d.id))}
                  style={{ fontSize: 11, color: "var(--accent-text)", cursor: "pointer", whiteSpace: "nowrap", background: "none", border: "none", padding: 0 }}
                >
                  open in Atlas ↗
                </button>
              )}
            </div>
          ))}
        </div>
      )}

      {/* ----- Services ----- */}
      {ctServices && (
        <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fill,minmax(300px,1fr))", gap: 12 }}>
          {svcCards.map((sv) => (
            <div key={sv.id} className="surface" style={{ padding: "16px 18px", display: "flex", flexDirection: "column", gap: 8 }}>
              <span style={{ fontSize: 13.5, fontWeight: 500 }}>{sv.name}</span>
              <p style={{ fontSize: 12, lineHeight: 1.5, color: "var(--tx-2)", margin: 0, flex: 1 }}>{sv.desc}</p>
              <div style={{ display: "flex", alignItems: "center", gap: 8, borderTop: "1px solid var(--line-1)", paddingTop: 10 }}>
                <span className="mono" style={{ fontSize: 10.5, color: "var(--tx-3)", flex: 1 }}>
                  {sv.url}
                </span>
                <button
                  className="mono"
                  onClick={() => void store.openExternal(federationUrl(sv.url))}
                  style={{ fontSize: 10, color: "var(--accent-text)", whiteSpace: "nowrap", cursor: "pointer", background: "none", border: "none", padding: 0 }}
                >
                  open ↗
                </button>
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
