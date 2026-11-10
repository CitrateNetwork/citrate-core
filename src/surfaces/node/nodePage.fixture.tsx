// Test fixture (imported only by *.test.tsx files): render the K3 Node page tabs to static markup in
// the desktop mode, with the reads the page would otherwise take from wagmi and the bridge. The
// public head is fresh and equal to the local height unless a test says otherwise, so a state the
// store calls "synced" reads Synced on the page too.
import { renderToStaticMarkup } from "react-dom/server";
import type { Store } from "../../shell/store";
import type { AppState } from "../../shell/state";
import { NodeStatusView, type NodeStatusViewProps } from "../Node";
import { NodeRewards } from "./NodeRewards";

/** A local height for a state whose node runs but whose test didn't set one (0 would read Starting). */
const RUNNING_HEIGHT = 510_000;

export function statusProps(store: Store, s: AppState, over: Partial<NodeStatusViewProps> = {}): NodeStatusViewProps {
  const running = s.node !== "off" && s.node !== "error" && s.node !== "paused" && s.node !== "prov";
  const st = running && s.height <= 0 ? { ...s, height: RUNNING_HEIGHT } : s;
  return {
    store,
    s: st,
    publicHead: st.height > 0 ? st.height : null,
    publicHeadAgeMs: st.height > 0 ? 1_000 : null,
    publicHeadError: false,
    statusAgeMs: 1_000,
    compat: "compatible",
    mode: "tauri",
    statusError: null,
    ...over,
  };
}

/** Node › Status as the desktop app renders it. */
export function renderStatus(store: Store, s: AppState, over: Partial<NodeStatusViewProps> = {}): string {
  return renderToStaticMarkup(<NodeStatusView {...statusProps(store, s, over)} />);
}

/** Node › Rewards as the desktop app renders it. */
export function renderRewards(store: Store, s: AppState): string {
  return renderToStaticMarkup(<NodeRewards store={store} s={s} mode="tauri" />);
}
