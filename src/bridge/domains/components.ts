// citrate-core: bridge domain `components`, moved verbatim from src/bridge/domains.ts by L0-WP05
// (planset v060). src/bridge/domains.ts re-exports it, so imports from "../bridge/domains" keep
// working. The owning lane is the row for this path in .agentile/v60-ownership.map.

// ---- HUP-S5.5 / S6.1 — signed first-run components. Mirrors Rust `components.rs`. ----

export interface ComponentsInstalled {
  name: string;
  version: string;
  previous: string | null;
  installedAt: number;
}

export interface ComponentsBundleTool {
  name: string;
  version: string;
  license: string;
  /** Platforms with a measured hash. */
  measuredPlatforms: string[];
  /** This machine's entry: measured | to_be_measured | to_be_built | upstream_unavailable | none. */
  thisPlatform: string;
}

export interface ComponentsStatus {
  /** false until the component signing key is set at the key ceremony; updates refuse until then. */
  keyConfigured: boolean;
  keyFingerprint: string | null;
  keyNote: string;
  platform: string | null;
  freshness: "never_checked" | "fresh" | "stale" | "expired";
  manifestAgeSecs: number | null;
  browserMayOpenWeb: boolean;
  installed: ComponentsInstalled[];
  bundle: ComponentsBundleTool[];
  libraries: { name: string; sha256: string }[];
  sla: {
    criticalHours: number;
    highDays: number;
    mediumDays: number;
    lowDays: number;
    staleAfterDays: number;
    pendingOwnerSignoff: boolean;
  };
  manifestUrl: string;
}

export interface ComponentsDomain {
  /** Read-only. null = no component store here (web preview), never an invented one. */
  status(): Promise<ComponentsStatus | null>;
  /** Verify-then-swap update of one component. Refuses while the key is not configured. */
  update(name: string): Promise<string>;
  rollback(name: string): Promise<string>;
}
