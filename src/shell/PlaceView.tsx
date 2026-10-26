// Five-place navigation (0.5.2) — the tab strip and the surface for the current place.
//
// This is composition only: every tab renders an existing surface (Dashboard, Agent, Groups, Node,
// Settings, …) unchanged. A tab with no content in this build renders <NotAvailableYet>, an honest
// statement of what is missing, never placeholder data (Rule 1). Routes and redirects live in
// places.ts; the map is docs/nav/FIVE_PLACE_ROUTE_MAP.md.
import type { ReactNode } from "react";
import type { Store } from "./store";
import type { AppState } from "./state";
import { ACCOUNT_SETTINGS, MOVED_CONTROLS_SETTINGS, NODE_SETTINGS, PLACES, locate, sectionFor, visibleTabs } from "./places";
import { Dashboard, Wallet, Node, Storage, Comms, Commissary, Settings, Journal, ALF } from "../surfaces";
import { ControlsPanel } from "../surfaces/hermes/ControlsPanel";
import { flRoundCardsEnabled } from "../fl/roundCardsFlag";
import { GroupRoundsCard } from "../fl/GroupRoundsCard";
import { FlRoundsPanel } from "../fl/FlRoundsPanel";
import { bridge } from "../bridge";
import { Models, StorageFiles, Groups, Cluster, Train, Agent, Connections, Community, People } from "../surfaces";
import { StoragePins, StorageBackup } from "../surfaces";

function TabStrip({ id, items, active, store, sub }: { id: string; items: { label: string; route: string }[]; active: string | null; store: Store; sub?: boolean }) {
  return (
    <div
      role="tablist"
      data-testid={id}
      style={{ display: "flex", gap: 4, padding: sub ? "6px 26px 0" : "10px 26px 0", borderBottom: sub ? "none" : "1px solid var(--line-1)", flexShrink: 0 }}
    >
      {items.map((t) => {
        const on = t.route === active;
        return (
          <button
            key={t.route}
            role="tab"
            aria-selected={on}
            onClick={() => store.go(t.route)}
            className="mono"
            style={{
              fontSize: sub ? 10.5 : 11,
              letterSpacing: ".06em",
              padding: sub ? "5px 10px" : "7px 12px",
              border: "none",
              borderBottom: sub ? "none" : on ? "2px solid var(--accent)" : "2px solid transparent",
              borderRadius: sub ? "var(--r-1)" : 0,
              background: sub && on ? "var(--srf-2)" : "transparent",
              color: on ? "var(--tx-1)" : "var(--tx-3)",
              cursor: "pointer",
            }}
          >
            {t.label}
          </button>
        );
      })}
    </div>
  );
}

/** The honest state for a tab whose content is not in this build. */
export function NotAvailableYet({ route, title, body, action }: { route: string; title: string; body: string; action?: { label: string; go: () => void } }) {
  return (
    <div data-not-available={route} style={{ padding: "20px 26px 24px", display: "flex", flexDirection: "column", gap: 16, boxSizing: "border-box" }}>
      <span style={{ fontFamily: "var(--font-display)", fontWeight: 420, fontSize: 24 }}>{title}</span>
      <div className="surface" style={{ padding: "18px 20px", display: "flex", flexDirection: "column", gap: 10, maxWidth: 620 }}>
        <span className="eyebrow">Not available yet</span>
        <p style={{ fontSize: 13, lineHeight: 1.6, color: "var(--tx-2)", margin: 0 }}>{body}</p>
        {action && (
          <span>
            <button className="btn btn-ghost btn-sm" onClick={action.go}>
              {action.label}
            </button>
          </span>
        )}
      </div>
    </div>
  );
}

/** Hermes › Controls: the Controls panel first, then the five Settings sections that moved here,
 *  unchanged (they write the same config they wrote under Settings). The panel is not repeated in
 *  the section list below it, so `settings/hermes` opens this tab on the first moved section. */
function HermesControlsSlot({ store, s }: { store: Store; s: AppState }) {
  return (
    <>
      <div style={{ padding: "20px 26px 0", maxWidth: 1000 }}>
        <ControlsPanel store={store} s={s} />
      </div>
      <Settings store={store} s={{ ...s, sSec: sectionFor(MOVED_CONTROLS_SETTINGS, s.sSec) }} sections={MOVED_CONTROLS_SETTINGS} heading="Controls" />
    </>
  );
}

/** Open a sub-view of the Node surface (its own Operations / Earning / Pinning switch). */
const openNodeTab = (store: Store, nTab: string) => () => {
  store.setState({ nTab });
  store.go("node/status");
};

function surfaceFor(route: string, store: Store, s: AppState): ReactNode {
  switch (route) {
    case "hermes/chat":
      return <Dashboard store={store} s={s} />;
    case "hermes/journal":
      return <Journal store={store} s={s} />;
    case "hermes/work":
      return <Agent store={store} s={s} />;
    case "hermes/learn":
      return (
        <NotAvailableYet
          route={route}
          title="Learn"
          body="One place for learned skills, Teach Hermes, knowledge import and the training opt-in is not available in this build. The skills Hermes proposes to keep are reviewed on the Work tab today."
          action={{ label: "Open Hermes › Work", go: () => store.go("hermes/work") }}
        />
      );
    case "hermes/controls":
      return <HermesControlsSlot store={store} s={s} />;
    case "groups/chat":
      return <Groups store={store} s={s} />;
    case "groups/chat/alerts":
      return <Comms store={store} s={s} />;
    case "groups/rounds":
      // FL round cards (0.5.2) behind VITE_FL_ROUND_CARDS: the address-book-gated ledger card in
      // place of Train's PatronageLedger read, with the same federated-rounds panel below it.
      if (flRoundCardsEnabled())
        return (
          <>
            <GroupRoundsCard variant="tab" />
            <div style={{ padding: "16px 26px 24px", maxWidth: 900 }}>
              <FlRoundsPanel fl={bridge.flRounds} requestSig={(spec) => store.requestSig(spec)} toast={(m) => store.toast(m)} />
            </div>
          </>
        );
      return <Train store={store} s={s} />;
    case "groups/members":
      return <People store={store} s={s} />;
    case "groups/cluster":
      return <Cluster store={store} s={s} />;
    case "groups/alf":
      return <ALF store={store} s={s} />;
    case "groups/community":
      return <Community store={store} s={s} />;
    case "wallet":
      return <Wallet store={store} s={s} />;
    case "node/status":
      return <Node store={store} s={s} />;
    case "node/services":
      return (
        <NotAvailableYet
          route={route}
          title="Services"
          body="A list of the processes this app supervises, each with its state, a restart control and a bounded log, is not available in this build. The node's own state is on the Status tab."
          action={{ label: "Open Node › Status", go: () => store.go("node/status") }}
        />
      );
    case "node/storage/files":
      return <StorageFiles store={store} s={s} />;
    case "node/storage/memory":
      return <Storage store={store} s={s} />;
    case "node/storage/pins":
      return <StoragePins store={store} s={s} />;
    case "node/storage/backup":
      return <StorageBackup store={store} s={s} />;
    case "node/rewards":
      return (
        <NotAvailableYet
          route={route}
          title="Rewards"
          body="One view of validator, pin and round rewards read from chain is not available in this build. What this build reads today is in the Earning view on Node › Status."
          action={{ label: "Open the Earning view", go: openNodeTab(store, "earn") }}
        />
      );
    case "node/advanced":
      return <Settings store={store} s={{ ...s, sSec: sectionFor(NODE_SETTINGS, s.sSec) }} sections={NODE_SETTINGS} heading="Advanced" />;
    case "store/apps":
      return <Commissary store={store} s={s} />;
    case "store/models":
      return <Models store={store} s={s} />;
    case "settings":
      // Every account-level section Settings owns opens here, Connections included. A moved section
      // (AI providers, Node configuration, …) falls back to Account.
      return <Settings store={store} s={{ ...s, sSec: sectionFor(ACCOUNT_SETTINGS, s.sSec) }} sections={ACCOUNT_SETTINGS} />;
    case "settings/connections":
      return <Connections store={store} s={s} />;
    default:
      return <Dashboard store={store} s={s} />;
  }
}

/** The place's tab strip (and sub-tab strip), then the scrolling surface for the current tab. */
export function PlaceView({ store, s }: { store: Store; s: AppState }) {
  const loc = locate(s.route);
  const place = PLACES.find((p) => p.id === loc.place);
  const tabs = place ? visibleTabs(place, s.alfMember) : [];
  const tab = tabs.find((t) => t.route === loc.tab);
  return (
    <>
      {tabs.length > 1 && <TabStrip id="place-tabs" items={tabs} active={loc.tab} store={store} />}
      {tab?.sub && <TabStrip id="place-subtabs" items={tab.sub} active={loc.sub} store={store} sub />}
      <div data-surface={loc.route} style={{ flex: 1, minHeight: 0, overflow: "auto" }}>
        {surfaceFor(loc.route, store, s)}
      </div>
    </>
  );
}
