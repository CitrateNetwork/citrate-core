// =====================================================================
// citrate-core — Commissary
// Ported 1:1 from design/CitrateCore.dc.html (COMMISSARY section + inline
// script: cTabs / appCards / sdkCards / docCards / svcCards; tier gating via
// RANK compare against effTier (FAIL-CLOSED — see rankOf); org-scoped cards
// HIDE on org mismatch — never shown as locked (Atlas doctrine).
//
// Data source — data/seed.ts CATALOG, a LOCAL SEED. It is not yet the signed
// manifest: the live fetch + JWKS verification is WS-G / CM-3, unbuilt.
//
// DOWNLOADS DO NOT RUN (Rule 1, QA 2026-08-01). The mint → dl → verify → done
// state machine was a setTimeout chain that verified nothing and then claimed
// "✓ verified · sha256 match · audit-logged". It was removed, not restyled —
// the same remedy Storage got for the same bug class. Cards list honestly as
// "not yet downloadable", which is also what the server returns today (409
// artifact_unreleased for every entry, with ARTIFACT_STORE_BASE unset).
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

/**
 * Rank a tier FAIL-CLOSED (QA 2026-08-01).
 *
 * The gate was `RANK[minTier] > RANK[effTier]`. A tier absent from RANK yields
 * `undefined`, and `2 > undefined` is false — so an UNRECOGNISED tier rendered every
 * gated card UNLOCKED. That is reachable, not theoretical: the identity authority
 * mints the ladder, this map is a client-side copy of it, and the two are known to
 * disagree. An unknown tier must collapse to the LEAST access, never the most.
 */
const rankOf = (tier: string | undefined): number => {
  const r = tier == null ? undefined : RANK[tier];
  return typeof r === "number" ? r : -1;
};

/**
 * A seeded checksum is a real digest only if it looks like one. The seed ships
 * ELIDED placeholders ("sha256:2f8e17aa…c9c41") which cannot verify anything;
 * rendering one in a mono font beside the word "verified" dresses a placeholder as
 * provenance (Rule 1). Show them as pending until a real release ledger fills them.
 */
const isRealDigest = (c: string | undefined): boolean =>
  typeof c === "string" && /^sha256:[0-9a-f]{64}$/i.test(c);

export function Commissary({ store, s }: SurfaceProps) {
  const effTier = s.entitlement === "lapsed" ? "free" : s.tier;

  // NO DOWNLOAD RUNS HERE YET (Rule 1, QA 2026-08-01).
  //
  // This was a setTimeout chain — mint → dl → verify → done — that streamed no
  // bytes, computed no digest, and wrote no audit row, then rendered
  // "✓ verified · sha256 match · audit-logged". Three false claims about
  // cryptographic verification that never happened. Storage carried the same bug
  // and it was removed there rather than dressed up (see storageHonesty.test.tsx);
  // this is the same remedy applied to the same bug class.
  //
  // The real client is WS-G / CM-3: fetch the EdDSA-signed manifest, verify it
  // against the JWKS, redeem the single-use URL, stream the bytes, and check the
  // sha256 before marking installed. It cannot serve real bytes until the artifact
  // store exists (`ARTIFACT_STORE_BASE` is unset), so until then the honest state
  // is "not downloadable yet" — which is also exactly what the server says: the
  // download route returns 409 `artifact_unreleased` for every catalog entry today.
  const startDownload = (id: string) => {
    store.setState((st0) => ({ dl: { ...st0.dl, [id]: { st: "unavailable", pct: 0 } } }));
    store.toast(
      "Downloads are not live yet — the artifact store is not provisioned, so there " +
        "are no verified bytes to serve. Listed, not yet downloadable.",
    );
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
      const locked = rankOf(a.minTier) > rankOf(effTier);
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
        checksum: isRealDigest(a.checksum) ? a.checksum : "checksum pending · no verified release",
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
        // No busy state exists while the client is unwired — a progress bar with
        // nothing behind it is the fabrication this pass removed.
        dlBusy: false,
        dlDone: false,
        dlUnavailable: dl.st === "unavailable",
        dlLabel: "",
        dlPctW: "0%",
        metaLine: isMicro ? caps.join(" · ") : "listed · not yet downloadable",
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
      const locked = rankOf(d.minTier) > rankOf(effTier);
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
                  {a.dlUnavailable && (
                    <span className="mono" style={{ fontSize: 10.5, color: "var(--warn)", flex: 1 }}>
                      not downloadable yet · artifact store not provisioned
                    </span>
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
