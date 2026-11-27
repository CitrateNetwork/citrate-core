// L0-WP06 (red step): the L0 spine's AppState part. Filled in the green commit.
import type { PersistScope } from "../netScope";

export interface CoreState {}

export function freshCore(_pid: string): CoreState {
  return {};
}

export const CORE_PERSIST_SCOPE = {} satisfies Partial<Record<keyof CoreState, PersistScope>>;
