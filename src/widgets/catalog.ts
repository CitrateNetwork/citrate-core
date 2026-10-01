// =====================================================================
// citrate-core — the widget query catalog (HUP-S10.3, US-10.3 AC1)
//
// Every piece of data a widget can ever ask for. Each query only READS state the main window
// already holds; none writes, signs, sends or calls a Tauri command. A widget declares the queries
// it needs when it is saved (Rust `widgets.rs` checks the names against its copy of this list, and
// a Rust test keeps the two lists equal), and the host answers only declared ones.
//
// Data sources (Rule 7):
//   node.status     store.snapshot(): height, peers, state, finality age (the node's local RPC)
//   wallet.summary  store.snapshot(): liquid / staked / claimable SALT (never the address)
//   model.active    the ModelRouter's active choice (the chat header's source)
//   daemons.summary the daemons slice (Rust daemons.rs via daemons_list)
// =====================================================================
export const WIDGET_QUERIES = ["node.status", "wallet.summary", "model.active", "daemons.summary"] as const;

export type WidgetQuery = (typeof WIDGET_QUERIES)[number];

export const QUERY_INFO: Readonly<Record<WidgetQuery, string>> = {
  "node.status": "your node's height, peers and sync state",
  "wallet.summary": "your liquid, staked and claimable SALT (not your address)",
  "model.active": "which model Hermes is using",
  "daemons.summary": "how many daemons you have and their states",
};

export function isWidgetQuery(v: unknown): v is WidgetQuery {
  return typeof v === "string" && (WIDGET_QUERIES as readonly string[]).includes(v);
}

/** The app state the catalog reads (store + slices in the app; plain functions in tests). */
export interface WidgetSources {
  context(): { height: number; peers: number; finalityAge: number; nodeState: string; staked: number; liquid: number; claimable: number };
  model(): { label: string; id: string | null };
  daemons(): { allPaused: boolean; total: number; running: number; paused: number; budgetUsedUp: number };
}

/** Resolve one query to plain JSON data. Throws when its source fails. */
export function resolveQuery(q: WidgetQuery, s: WidgetSources): unknown {
  switch (q) {
    case "node.status": {
      const c = s.context();
      return { height: c.height, peers: c.peers, state: c.nodeState, finalityAgeSec: c.finalityAge };
    }
    case "wallet.summary": {
      const c = s.context();
      return { liquidSalt: c.liquid, stakedSalt: c.staked, claimableSalt: c.claimable };
    }
    case "model.active": {
      const m = s.model();
      return { label: m.label, id: m.id };
    }
    case "daemons.summary": {
      const d = s.daemons();
      return { allPaused: d.allPaused, total: d.total, running: d.running, paused: d.paused, budgetUsedUp: d.budgetUsedUp };
    }
  }
}
