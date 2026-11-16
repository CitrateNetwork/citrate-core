// =====================================================================
// citrate-core: Groups › Rounds and its sub-tabs (FL design, Q2)
//
// The hub keeps the 0.5.4 shape: five places plus Settings, with Rounds under Groups (the legacy
// `train` route redirects here). Sub-tabs: Matches, Live round, Open a round, Datasets, Adapters,
// Payouts (places.ts). Each sub-tab renders its board; a board that needs rounds renders the honest
// "Today" state from the network state model while the data shows no rounds.
// =====================================================================
import type { Store } from "../shell/store";
import type { AppState } from "../shell/state";
import { bridge } from "../bridge";
import { Train } from "../surfaces";
import { flRoundCardsEnabled } from "./roundCardsFlag";
import { GroupRoundsCard } from "./GroupRoundsCard";
import { FlRoundsPanel } from "./FlRoundsPanel";
import { FlBand } from "./FlBand";
import { RoundsNotLive } from "./RoundsNotLive";
import { useFlNetworkState } from "./useFlNetworkState";
import { NOT_AVAILABLE, PAYOUTS_DARK } from "./networkState";

export const ROUNDS_ROUTES = {
  matches: "groups/rounds",
  live: "groups/rounds/live",
  open: "groups/rounds/open",
  datasets: "groups/rounds/datasets",
  adapters: "groups/rounds/adapters",
  payouts: "groups/rounds/payouts",
} as const;

export function isRoundsRoute(route: string): boolean {
  return (Object.values(ROUNDS_ROUTES) as string[]).includes(route);
}

function Matches({ store, s }: { store: Store; s: AppState }) {
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
}

function NotLiveSubtab({ route, s }: { route: string; s: AppState }) {
  const { state } = useFlNetworkState();
  const band = <FlBand s={s} networkLabel={state.networkLabel} />;
  switch (route) {
    case ROUNDS_ROUTES.live:
      return (
        <>
          {band}
          <RoundsNotLive testid="rounds-live-not-live" state={state} headline="There is no live round to show on this network yet." lead="A live round appears here once you join one. Rounds are offered through the WorkMarket contract." />
        </>
      );
    case ROUNDS_ROUTES.open:
      return (
        <RoundsNotLive
          testid="rounds-open-not-live"
          state={state}
          headline="Opening a round needs the WorkMarket contract on this network."
          lead="Owners and admins of a group open rounds. Until WorkMarket is deployed there is no round to open and nothing to sign."
        />
      );
    case ROUNDS_ROUTES.datasets:
      return (
        <RoundsNotLive
          testid="rounds-datasets-not-live"
          state={state}
          contracts={["DatasetRegistry", "WorkMarket"]}
          headline="Publishing a dataset needs the DatasetRegistry contract on this network."
          lead="Datasets stay on this device. Recording one on chain waits for DatasetRegistry."
        />
      );
    case ROUNDS_ROUTES.payouts:
      return (
        <RoundsNotLive
          testid="rounds-payouts-not-live"
          state={state}
          contracts={["WorkMarket", "PatronageLedger"]}
          headline={state.payouts === "not-deployed" ? `Payouts: ${NOT_AVAILABLE}` : PAYOUTS_DARK}
          lead="Payouts are paid by WorkMarket in SALT. Nothing here is claimable until rounds run and the security review opens payouts."
        />
      );
    default:
      return null;
  }
}

export function RoundsRoute({ route, store, s }: { route: string; store: Store; s: AppState }) {
  switch (route) {
    case ROUNDS_ROUTES.matches:
      return <Matches store={store} s={s} />;
    case ROUNDS_ROUTES.adapters:
      // The eval gate and adapter load, unload and re-apply on restart (fl_rounds.rs).
      return (
        <div style={{ padding: "20px 26px 24px", maxWidth: 900 }}>
          <FlRoundsPanel fl={bridge.flRounds} requestSig={(spec) => store.requestSig(spec)} toast={(m) => store.toast(m)} />
        </div>
      );
    default:
      return <NotLiveSubtab route={route} s={s} />;
  }
}
