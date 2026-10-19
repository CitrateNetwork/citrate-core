// =====================================================================
// citrate-core — Store catalog, listing page and install sheet (K5a)
//
// Data source: bridge.store → Rust `store_catalog.rs`. The catalog is a signed document verified
// against the catalog key from the key ceremony. Until that key is issued, core answers "no_key"
// before any network or disk access, and this screen says so; it never shows a listing that did
// not verify. Install path: core stages the listing (signature, and a daemon's hash, checked
// now), this sheet shows the declared permissions and budget, the member approves, and the
// listing installs paused. Removal is one call.
//
// Honest states: no key, fetch failed, signature failed, catalog invalid / expired / older than
// the one already seen, hash failed, tier-locked, not installable yet, desktop only.
// =====================================================================
import { useEffect } from "react";
import { ModalDialog } from "../shell/ModalDialog";
import {
  storeSlice,
  refreshStore,
  openListing,
  beginInstall,
  approveInstall,
  closeSheet,
  removeInstall,
  startInstalledDaemon,
  type StoreSheet,
} from "../shell/slices/store";
import type { StoreCatalogView, StoreInstalled, StoreListing, StoreRefusal, StoreBudget, StorePermission } from "../bridge/domains";

export const KIND_LABEL: Record<string, string> = {
  app: "App",
  daemon: "Daemon",
  widget: "Widget",
  skill: "Skill",
  model: "Model",
  mcp_server: "MCP server",
};

/** Plain words for the permission ids core knows; any other id is shown as is, with its reason. */
export const PERMISSION_LABEL: Record<string, string> = {
  local_model: "Use your local model",
  files_read: "Read the folders you granted",
  files_write: "Change files in the folders you granted",
  web_search: "Search the web",
  web_browse: "Open web pages",
  memory_read: "Read your memory",
  memory_write: "Add to your memory",
  chain_read: "Read the chain",
  network: "Use the network",
};

/** The catalog-level states, in member words. */
export const CATALOG_STATE_TEXT: Record<string, { title: string; body: string }> = {
  no_key: {
    title: "Store catalog not available yet",
    body: "The catalog signing key has not been issued. Nothing is listed until a catalog signed with that key can be verified.",
  },
  fetch_failed: {
    title: "Could not reach the Store catalog",
    body: "The signed catalog could not be downloaded. Nothing is listed from an unverified source.",
  },
  signature_failed: {
    title: "The Store catalog failed its signature check",
    body: "The catalog did not verify against the pinned catalog key, so none of it is shown.",
  },
  invalid: {
    title: "The Store catalog is not valid",
    body: "It verified, but a field is out of bounds, so none of it is shown.",
  },
  expired: {
    title: "The Store catalog has expired",
    body: "A signed catalog is trusted for a limited time. Check the clock, or try again later.",
  },
  rollback: {
    title: "The Store catalog is older than the one already seen",
    body: "An older catalog is refused so a withdrawn listing cannot come back.",
  },
  desktop_only: {
    title: "The Store needs the desktop app",
    body: "This preview cannot verify the signed catalog.",
  },
};

/** Install-sheet refusals, in member words (the core's message is shown under it). */
export const REFUSAL_TITLE: Record<string, string> = {
  no_key: "Store catalog not available yet",
  fetch_failed: "Could not download it",
  signature_failed: "Signature check failed",
  invalid: "The listing is not valid",
  expired: "The Store catalog has expired",
  rollback: "The Store catalog is older than the one already seen",
  hash_failed: "Hash check failed: nothing was installed",
  tier_locked: "Not included in your membership",
  not_installable: "Not installable yet",
  not_listed: "No longer listed",
  stage_expired: "This review expired",
  changed: "The listing changed",
  already_installed: "Already installed",
  not_installed: "Not installed",
  in_use: "In use",
  install_failed: "The install did not finish",
  desktop_only: "The Store needs the desktop app",
};

export function humanSize(n: number): string {
  if (!Number.isFinite(n) || n <= 0) return "—";
  if (n >= 1e9) return `${(n / 1e9).toFixed(1)} GB`;
  if (n >= 1e6) return `${(n / 1e6).toFixed(1)} MB`;
  if (n >= 1e3) return `${Math.round(n / 1e3)} KB`;
  return `${n} bytes`;
}

const cap = { fontSize: 10, letterSpacing: ".08em", textTransform: "uppercase" as const, color: "var(--tx-3)" };

export function CatalogStateBanner({ view, error }: { view: StoreCatalogView | null; error: string | null }) {
  if (error) {
    return (
      <div className="surface" role="status" style={{ padding: "14px 18px", borderColor: "var(--warn)" }}>
        <span style={{ display: "block", fontSize: 13.5, fontWeight: 500 }}>The Store could not be read</span>
        <span style={{ display: "block", fontSize: 12, color: "var(--tx-2)", marginTop: 4 }}>{error}</span>
      </div>
    );
  }
  if (!view || view.state === "ready") return null;
  const t = CATALOG_STATE_TEXT[view.state] ?? { title: "The Store catalog is not available", body: "" };
  return (
    <div className="surface" role="status" data-store-state={view.state} style={{ padding: "14px 18px" }}>
      <span style={{ display: "block", fontSize: 13.5, fontWeight: 500 }}>{t.title}</span>
      <span style={{ display: "block", fontSize: 12, color: "var(--tx-2)", marginTop: 4, lineHeight: 1.5 }}>{t.body}</span>
      {view.message && (
        <span className="mono" style={{ display: "block", fontSize: 10.5, color: "var(--tx-3)", marginTop: 6 }}>
          {view.message}
        </span>
      )}
    </div>
  );
}

export function PermissionList({ permissions }: { permissions: StorePermission[] }) {
  if (permissions.length === 0) return <span style={{ fontSize: 12, color: "var(--tx-3)" }}>Declares no permissions.</span>;
  return (
    <ul style={{ margin: 0, paddingLeft: 18, display: "flex", flexDirection: "column", gap: 4 }}>
      {permissions.map((p) => (
        <li key={p.id} style={{ fontSize: 12.5, lineHeight: 1.45 }}>
          <span style={{ fontWeight: 500 }}>{PERMISSION_LABEL[p.id] ?? p.id}</span>
          <span style={{ color: "var(--tx-2)" }}> · {p.reason}</span>
        </li>
      ))}
    </ul>
  );
}

export function BudgetLine({ budget }: { budget: StoreBudget | null }) {
  if (!budget) return <span style={{ fontSize: 12, color: "var(--tx-3)" }}>No budget: this kind does not run on a schedule.</span>;
  return (
    <span style={{ fontSize: 12.5 }}>
      Up to {budget.maxRunsPerDay} runs a day · {budget.maxTokensPerDay.toLocaleString()} tokens a day ·{" "}
      {budget.maxTokensPerRun.toLocaleString()} tokens a run · spend {budget.maxSpendSalt} SALT
    </span>
  );
}

export function ListingCard({ l, onOpen }: { l: StoreListing; onOpen: () => void }) {
  return (
    <button
      className="surface"
      onClick={onOpen}
      style={{ padding: 16, display: "flex", flexDirection: "column", gap: 8, textAlign: "left", cursor: "pointer", font: "inherit", color: "inherit" }}
    >
      <span style={{ display: "flex", alignItems: "center", gap: 8 }}>
        <span style={{ fontSize: 14, fontWeight: 500, flex: 1 }}>{l.name}</span>
        <span className="mono" style={{ ...cap, border: "1px solid var(--line-1)", borderRadius: 999, padding: "2px 8px" }}>
          {KIND_LABEL[l.kind] ?? l.kind}
        </span>
      </span>
      <span style={{ fontSize: 12.5, lineHeight: 1.5, color: "var(--tx-2)" }}>{l.summary}</span>
      <span className="mono" style={{ fontSize: 10.5, color: "var(--tx-3)" }}>
        v{l.version} · {l.publisher}
        {l.firstParty ? " · first-party" : ""} · {humanSize(l.sizeBytes)}
      </span>
      <span className="mono" style={{ fontSize: 10.5, color: l.locked ? "var(--warn)" : "var(--tx-3)" }}>
        {l.installed ? "installed" : l.locked ? l.lockReason : l.installable ? "signature verified · installs paused" : l.installNote}
      </span>
    </button>
  );
}

export function ListingPage({
  l,
  installed,
  onBack,
  onInstall,
}: {
  l: StoreListing;
  installed: StoreInstalled | null;
  onBack: () => void;
  onInstall: () => void;
}) {
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 14, maxWidth: 760 }}>
      <button className="btn btn-ghost btn-sm" onClick={onBack} style={{ alignSelf: "flex-start" }}>
        ← All listings
      </button>
      <div className="surface" style={{ padding: 20, display: "flex", flexDirection: "column", gap: 12 }}>
        <span style={{ display: "flex", alignItems: "center", gap: 10 }}>
          <span style={{ fontFamily: "var(--font-display)", fontSize: 20, flex: 1 }}>{l.name}</span>
          <span className="mono" style={cap}>
            {KIND_LABEL[l.kind] ?? l.kind} · {l.tier}
          </span>
        </span>
        <span style={{ fontSize: 13, lineHeight: 1.55, color: "var(--tx-2)" }}>{l.summary}</span>
        <span className="mono" style={{ fontSize: 10.5, color: "var(--tx-3)", wordBreak: "break-all" }}>
          v{l.version} · {l.publisher}
          {l.firstParty ? " · first-party" : ""} · {humanSize(l.sizeBytes)} · sha256 {l.sha256}
        </span>
        <span style={cap}>Declared permissions</span>
        <PermissionList permissions={l.permissions} />
        <span style={cap}>Declared budget</span>
        <BudgetLine budget={l.budget} />
        <div style={{ borderTop: "1px solid var(--line-1)", paddingTop: 12, display: "flex", alignItems: "center", gap: 10 }}>
          {installed ? (
            <span style={{ fontSize: 12.5, flex: 1 }}>Installed · {INSTALLED_STATE_TEXT[installed.state] ?? installed.state}</span>
          ) : l.locked ? (
            <span style={{ fontSize: 12.5, color: "var(--warn)", flex: 1 }}>{l.lockReason}</span>
          ) : !l.installable ? (
            <span style={{ fontSize: 12.5, color: "var(--tx-3)", flex: 1 }}>{l.installNote}</span>
          ) : (
            <>
              <span style={{ fontSize: 12, color: "var(--tx-3)", flex: 1 }}>Review what it asks for before it installs. It arrives paused.</span>
              <button className="btn btn-primary btn-sm" onClick={onInstall}>
                Review and install
              </button>
            </>
          )}
        </div>
      </div>
    </div>
  );
}

export const INSTALLED_STATE_TEXT: Record<string, string> = {
  paused: "paused, does nothing until you start it",
  active: "started",
  not_selected: "on this machine, not in use until you select it in Models",
  missing: "removed outside the Store",
};

function hashLine(staged: { hashCheck: string }): string {
  return staged.hashCheck === "verified"
    ? "Catalog signature verified. The download's SHA-256 matched the signed catalog."
    : "Catalog signature verified. The SHA-256 is checked as it downloads; a file that does not match is not kept.";
}

function pausedLine(kind: string): string {
  return kind === "model"
    ? "It downloads and is verified, then waits: it is not used until you select it in Models."
    : "It installs paused and does nothing until you start it.";
}

/** The install sheet, one view per phase. Presentational: the caller wires the actions. */
export function InstallSheetView({
  sheet,
  listingName,
  onApprove,
  onClose,
}: {
  sheet: StoreSheet;
  listingName: string;
  onApprove: () => void;
  onClose: () => void;
}) {
  const busy = sheet.phase === "staging" || sheet.phase === "installing";
  return (
    <ModalDialog
      register="instrument"
      zIndex={58}
      labelledBy="store-sheet-title"
      describedBy="store-sheet-body"
      onEscape={busy ? undefined : onClose}
      focusKey={"store-sheet:" + sheet.listingId + ":" + sheet.phase}
      panelStyle={{ width: 480, maxWidth: "100%", background: "var(--srf-0, #fff)", borderRadius: 12, padding: "20px 22px", display: "flex", flexDirection: "column", gap: 12 }}
    >
      <span id="store-sheet-title" style={{ fontSize: 15, fontWeight: 560 }}>
        {sheet.phase === "done" ? `${listingName} installed` : sheet.phase === "refused" ? REFUSAL_TITLE[sheet.refusal?.code ?? ""] ?? "Not installed" : `Install ${listingName}?`}
      </span>
      <div id="store-sheet-body" data-sheet-phase={sheet.phase} style={{ display: "flex", flexDirection: "column", gap: 10, fontSize: 12.5, lineHeight: 1.55 }}>
        {sheet.phase === "staging" && <span style={{ color: "var(--tx-2)" }}>Checking the signature and the download against the signed catalog…</span>}
        {sheet.phase === "review" && sheet.staged && (
          <>
            <span className="mono" style={{ fontSize: 10.5, color: "var(--tx-3)" }}>{hashLine(sheet.staged)}</span>
            <span style={cap}>It asks for</span>
            <PermissionList permissions={sheet.staged.permissions} />
            <span style={cap}>Its budget</span>
            <BudgetLine budget={sheet.staged.budget} />
            <span style={{ color: "var(--tx-2)" }}>{pausedLine(sheet.staged.kind)}</span>
          </>
        )}
        {sheet.phase === "installing" && (
          <span style={{ color: "var(--tx-2)" }}>
            Installing{sheet.pct != null ? ` · ${sheet.pct}% downloaded` : "…"}
          </span>
        )}
        {sheet.phase === "done" && sheet.installed && (
          <span>
            {INSTALLED_STATE_TEXT[sheet.installed.state] ? `Installed · ${INSTALLED_STATE_TEXT[sheet.installed.state]}.` : "Installed."}
          </span>
        )}
        {sheet.phase === "refused" && sheet.refusal && <RefusalBody refusal={sheet.refusal} />}
      </div>
      <div style={{ display: "flex", justifyContent: "flex-end", gap: 8 }}>
        {sheet.phase === "review" ? (
          <>
            <button className="btn btn-ghost btn-sm" data-autofocus onClick={onClose}>
              Not now
            </button>
            <button className="btn btn-primary btn-sm" onClick={onApprove}>
              Approve and install paused
            </button>
          </>
        ) : (
          <button className="btn btn-ghost btn-sm" data-autofocus onClick={onClose} disabled={busy}>
            {sheet.phase === "done" ? "Done" : "Close"}
          </button>
        )}
      </div>
    </ModalDialog>
  );
}

function RefusalBody({ refusal }: { refusal: StoreRefusal }) {
  return (
    <>
      <span>{refusal.message}</span>
      {refusal.code === "hash_failed" && (
        <span style={{ color: "var(--tx-2)" }}>The download did not match the signed catalog, so nothing was installed.</span>
      )}
      {(refusal.code === "stage_expired" || refusal.code === "changed") && (
        <span style={{ color: "var(--tx-2)" }}>Close this and review the listing again.</span>
      )}
    </>
  );
}

export function InstalledList({ items, removing }: { items: StoreInstalled[]; removing: string | null }) {
  if (items.length === 0) return null;
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 8, maxWidth: 760 }}>
      <span style={cap}>Installed from the Store</span>
      {items.map((i) => (
        <div key={i.listingId} className="surface" style={{ padding: "12px 16px", display: "flex", alignItems: "center", gap: 10 }}>
          <span style={{ flex: 1, minWidth: 0 }}>
            <span style={{ display: "block", fontSize: 13, fontWeight: 500 }}>{i.name}</span>
            <span style={{ display: "block", fontSize: 11.5, color: "var(--tx-3)" }}>
              {KIND_LABEL[i.kind] ?? i.kind} · v{i.version} · {INSTALLED_STATE_TEXT[i.state] ?? i.state}
            </span>
          </span>
          {i.kind === "daemon" && i.state === "paused" && (
            <button className="btn btn-secondary btn-sm" onClick={() => void startInstalledDaemon(i.target)}>
              Start
            </button>
          )}
          <button className="btn btn-ghost btn-sm" disabled={removing === i.listingId} onClick={() => void removeInstall(i.listingId)}>
            {removing === i.listingId ? "Removing…" : "Remove"}
          </button>
        </div>
      ))}
    </div>
  );
}

/** The Store's signed-catalog tab: list → listing page → install sheet. */
export function StoreCatalog() {
  const st = storeSlice.use();
  useEffect(() => {
    void refreshStore();
  }, []);
  const view = st.catalog;
  const listings = view?.state === "ready" ? view.listings : [];
  const open = st.openId ? listings.find((l) => l.id === st.openId) ?? null : null;
  const sheetListing = st.sheet ? listings.find((l) => l.id === st.sheet?.listingId) ?? null : null;

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 14 }}>
      {st.loading && !view && <span style={{ fontSize: 12, color: "var(--tx-3)" }}>Checking the signed catalog…</span>}
      <CatalogStateBanner view={view} error={st.error} />
      {st.notice && (
        <span role="status" style={{ fontSize: 12, color: "var(--tx-2)" }}>
          {st.notice}
        </span>
      )}
      {open ? (
        <ListingPage
          l={open}
          installed={st.installed.find((i) => i.listingId === open.id) ?? null}
          onBack={() => openListing(null)}
          onInstall={() => void beginInstall(open.id)}
        />
      ) : (
        view?.state === "ready" && (
          <>
            {listings.length === 0 ? (
              <span style={{ fontSize: 12.5, color: "var(--tx-3)" }}>The signed catalog has no listings yet.</span>
            ) : (
              <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fill,minmax(300px,1fr))", gap: 12 }}>
                {listings.map((l) => (
                  <ListingCard key={l.id} l={l} onOpen={() => openListing(l.id)} />
                ))}
              </div>
            )}
            <span className="mono" style={{ fontSize: 10, color: "var(--tx-3)" }}>
              signed catalog · sequence {view.sequence ?? "—"} · key {view.keyFingerprint ?? "—"}
            </span>
          </>
        )
      )}
      <InstalledList items={st.installed} removing={st.removing} />
      {st.sheet && (
        <InstallSheetView
          sheet={st.sheet}
          listingName={sheetListing?.name ?? st.sheet.staged?.name ?? st.sheet.listingId}
          onApprove={() => void approveInstall()}
          onClose={closeSheet}
        />
      )}
    </div>
  );
}
