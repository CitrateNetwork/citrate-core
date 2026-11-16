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
import { useEffect, useState } from "react";
import { tierSlice, refreshTier } from "../shell/slices/tier";
import { useAuthority } from "./authority";
import { loadWorkerTerms, type WorkerTerms } from "./workerTerms";
import { machineFrom, readMarket } from "./market";
import { JOIN_SIGNATURE, joinCalldata, MatchesView, useMarket } from "./Matches";
import { downloadText } from "./download";
import { AdaptersView } from "./Adapters";
import { NoLiveRound } from "./LiveRound";
import { PayoutsView, usePayoutsRecords } from "./Payouts";
import { OpenRoundView } from "./OpenRound";
import { DatasetsView } from "./Datasets";
import { BLANK_DRAFT, loadRoundDraft, type RoundDraft } from "./roundDraft";
import { groupsSlice } from "../shell/slices/groups";
import { flRoundCardsEnabled } from "./roundCardsFlag";
import { GroupRoundsCard } from "./GroupRoundsCard";
import { FlRoundsPanel } from "./FlRoundsPanel";
import { FlBand } from "./FlBand";
import { RoundsNotLive } from "./RoundsNotLive";
import { useFlNetworkState } from "./useFlNetworkState";
import type { FlNetworkState } from "./networkState";

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
  // FL round cards (0.5.2) behind VITE_FL_ROUND_CARDS stay as they were.
  if (flRoundCardsEnabled())
    return (
      <>
        <GroupRoundsCard variant="tab" />
        <div style={{ padding: "16px 26px 24px", maxWidth: 900 }}>
          <FlRoundsPanel fl={bridge.flRounds} requestSig={(spec) => store.requestSig(spec)} toast={(m) => store.toast(m)} />
        </div>
      </>
    );
  return <MatchesRoute store={store} s={s} />;
}

function MatchesRoute({ store, s }: { store: Store; s: AppState }) {
  const { state } = useFlNetworkState();
  const tier = tierSlice.use();
  const authority = useAuthority();
  const [terms, setTerms] = useState<WorkerTerms | null>(null);
  const market = useMarket(state, readMarket);
  useEffect(() => {
    if (!tier.loaded) void refreshTier();
    void loadWorkerTerms().then(setTerms, () => setTerms(null));
  }, []);
  const work = state.contracts.WorkMarket;
  return (
    <MatchesView
      state={state}
      band={<FlBand s={s} networkLabel={state.networkLabel} />}
      market={market}
      terms={terms}
      machine={machineFrom(tier.report?.facts)}
      authority={authority}
      go={(r) => store.go(r)}
      save={(name, text) => downloadText(name, text)}
      onJoin={async (o) => {
        if (work.kind !== "deployed") throw new Error("WorkMarket is not deployed on this network");
        await store.proposeContractCall({ address: work.address, calldata: joinCalldata(o), valueWei: o.terms?.joinBond ?? "0", label: JOIN_SIGNATURE });
      }}
      coordinator={<FlRoundsPanel bare sections={["coordinator", "plan", "consents"]} fl={bridge.flRounds} requestSig={(spec) => store.requestSig(spec)} toast={(m) => store.toast(m)} />}
    />
  );
}

function PayoutsRoute({ state, s, go }: { state: FlNetworkState; s: AppState; go: (r: string) => void }) {
  const { ov, error } = usePayoutsRecords(bridge.flRounds);
  return (
    <PayoutsView
      state={state}
      receipts={null}
      receiptsReason="this build has no settled-lines read model yet"
      pullWei={null}
      pullReason="this build does not read the WorkMarket pull balance yet"
      wallet={s.walletAddr}
      ov={ov}
      ovError={error}
      go={go}
      save={(name, text) => downloadText(name, text)}
      act={async () => {
        throw new Error("claims, withdrawals and disputes need the settled-lines read model, which is not in this build");
      }}
    />
  );
}

function OpenRoundRoute({ state, s, go }: { state: FlNetworkState; s: AppState; go: (r: string) => void }) {
  const { groups } = groupsSlice.use();
  const [coord, setCoord] = useState<string | null>(null);
  const [draft] = useState<RoundDraft>(() => loadRoundDraft() ?? BLANK_DRAFT);
  useEffect(() => {
    bridge.flRounds.overview().then(
      (o) => setCoord(o.config.url),
      () => setCoord(null),
    );
  }, []);
  const roleOf = (id: string | null) => {
    const g = groups.find((x) => x.id === id);
    const me = g && s.commsAddr ? g.members.find((m) => m.address.toLowerCase() === s.commsAddr.toLowerCase()) : undefined;
    return me ? me.role : null;
  };
  return (
    <OpenRoundView
      state={state}
      draft={draft}
      groups={groups}
      roleOf={roleOf}
      coordinatorUrl={coord}
      go={go}
      onOpen={async () => {
        throw new Error("opening needs the round's requirements file pinned to IPFS and WorkMarket.open, which this build does not do yet");
      }}
    />
  );
}

function NotLiveSubtab({ route, s, go, toast }: { route: string; s: AppState; go: (r: string) => void; toast: (m: string) => void }) {
  const { state } = useFlNetworkState();
  const band = <FlBand s={s} networkLabel={state.networkLabel} />;
  switch (route) {
    case ROUNDS_ROUTES.live:
      // Board 06: the live round view (LiveRound.tsx) needs a joined round; this build has no
      // per-lease read model, so with WorkMarket deployed it says there is none to show.
      return (
        <>
          {band}
          {state.phase === "rounds-live" ? (
            <NoLiveRound go={go} />
          ) : (
            <RoundsNotLive testid="rounds-live-not-live" state={state} headline="There is no live round to show on this network yet." lead="A live round appears here once you join one. Rounds are offered through the WorkMarket contract." />
          )}
        </>
      );
    case ROUNDS_ROUTES.open:
      // Board 07: the four steps; no Open button until WorkMarket is on this network.
      return <OpenRoundRoute state={state} s={s} go={go} />;
    case ROUNDS_ROUTES.datasets:
      // Board 08: local datasets (core's training-set builder) and the honest states for the rest.
      return <DatasetsView state={state} fl={bridge.flRounds} toast={toast} go={go} />;
    case ROUNDS_ROUTES.payouts:
      // Board 10: receipts (dark or not deployed in this build) and the device's history.
      return <PayoutsRoute state={state} s={s} go={go} />;
    default:
      return null;
  }
}

export function RoundsRoute({ route, store, s }: { route: string; store: Store; s: AppState }) {
  switch (route) {
    case ROUNDS_ROUTES.matches:
      return <Matches store={store} s={s} />;
    case ROUNDS_ROUTES.adapters:
      // Board 09: the eval gate and adapter load, unload and re-apply on restart (fl_rounds.rs).
      return <AdaptersView fl={bridge.flRounds} toast={(m) => store.toast(m)} go={(r) => store.go(r)} />;
    default:
      return <NotLiveSubtab route={route} s={s} go={(r) => store.go(r)} toast={(m) => store.toast(m)} />;
  }
}
