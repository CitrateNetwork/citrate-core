// L0-WP06 (red step): lane L2's AppState part. Filled in the green commit.
import type { PersistScope } from "../netScope";

export interface L2State {}

export function freshL2(_pid: string): L2State {
  return {};
}

export const L2_PERSIST_SCOPE = {} satisfies Partial<Record<keyof L2State, PersistScope>>;
