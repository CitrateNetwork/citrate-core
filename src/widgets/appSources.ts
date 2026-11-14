// =====================================================================
// citrate-core — the live sources behind the widget query catalog (HUP-S10.3)
//
// The same state the rest of the app shows: the store's agent snapshot (node + wallet), the model
// router's active choice (as the chat header and the Activity monitor resolve it), and the daemons
// slice. Reading these never calls a Tauri command.
// =====================================================================
import { store } from "../shell/store";
import { modelsSlice } from "../shell/slices/models";
import { routerChoicesFor } from "../agent/modelRouterSources";
import { activeRouterId, resolveActive } from "../agent/modelRouter";
import { daemonsSlice, daemonsSummary } from "../daemons/slice";
import type { WidgetSources } from "./catalog";

export const appWidgetSources: WidgetSources = {
  context: () => store.snapshot(),
  model: () => {
    const models = modelsSlice.get();
    const active = resolveActive(activeRouterId(store.state.activeModelId, store.state.hermesCloudModel), routerChoicesFor(models, store.state.chatRoute));
    return { label: active.label, id: active.id };
  },
  daemons: () => daemonsSummary(daemonsSlice.get().view),
};
