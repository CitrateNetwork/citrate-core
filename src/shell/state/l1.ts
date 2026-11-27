// L0-WP06 (red step): lane L1's AppState part. Filled in the green commit.
import type { PersistScope } from "../netScope";

export interface L1State {}

export function freshL1(_pid: string): L1State {
  return {};
}

export const L1_PERSIST_SCOPE = {} satisfies Partial<Record<keyof L1State, PersistScope>>;
