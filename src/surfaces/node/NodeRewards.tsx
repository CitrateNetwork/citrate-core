// K3 red-first stub (replaced by the Node › Rewards tab in the next commit). It keeps the old
// behaviour: the 0.5.1 Node screen's Earning view.
import type { Store } from "../../shell/store";
import type { AppState } from "../../shell/state";
import { Node } from "../Node";

export function NodeRewards({ store, s }: { store: Store; s: AppState; mode?: "tauri" | "sim" }) {
  return <Node store={store} s={{ ...s, nTab: "earn" }} />;
}
