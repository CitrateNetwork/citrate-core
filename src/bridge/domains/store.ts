// citrate-core: bridge domain `store`, moved verbatim from src/bridge/domains.ts by L0-WP05
// (planset v060). src/bridge/domains.ts re-exports it, so imports from "../bridge/domains" keep
// working. The owning lane is the row for this path in .agentile/v60-ownership.map.

// ---- K5a — Store v1: signed catalog, staged installs that arrive paused. Mirrors Rust
// `store_catalog.rs`. Every catalog call refuses until the catalog key is issued at the key
// ceremony ("no_key"), before any network or disk access. ----

/** `ready`, or why the catalog is not shown (one honest state each). */
export type StoreCatalogState =
  | "ready"
  | "no_key"
  | "fetch_failed"
  | "signature_failed"
  | "invalid"
  | "expired"
  | "rollback"
  | "desktop_only";

/** Why a Store call did not go ahead. */
export type StoreRefusalCode =
  | "no_key"
  | "fetch_failed"
  | "signature_failed"
  | "invalid"
  | "expired"
  | "rollback"
  | "hash_failed"
  | "tier_locked"
  | "not_installable"
  | "not_listed"
  | "stage_expired"
  | "changed"
  | "already_installed"
  | "not_installed"
  | "in_use"
  | "install_running"
  | "daemon_running"
  | "install_failed"
  | "desktop_only";

export interface StoreRefusal {
  code: StoreRefusalCode;
  message: string;
}

/** Exactly one of `ok` / `refused`. */
export interface StoreOutcome<T> {
  ok?: T;
  refused?: StoreRefusal;
}

export type StoreListingKind = "app" | "daemon" | "widget" | "skill" | "model" | "mcp_server";

export interface StorePermission {
  id: string;
  reason: string;
}

export interface StoreBudget {
  maxRunsPerDay: number;
  maxTokensPerDay: number;
  maxTokensPerRun: number;
  /** Always "0" in this version. */
  maxSpendSalt: string;
}

export interface StoreListing {
  id: string;
  kind: StoreListingKind;
  name: string;
  summary: string;
  version: string;
  publisher: string;
  firstParty: boolean;
  tier: "public" | "member" | "enterprise";
  sizeBytes: number;
  sha256: string;
  permissions: StorePermission[];
  budget: StoreBudget | null;
  locked: boolean;
  lockReason: string | null;
  installable: boolean;
  installNote: string | null;
  installed: boolean;
}

export interface StoreCatalogView {
  state: StoreCatalogState;
  message: string | null;
  catalogUrl: string;
  keyFingerprint: string | null;
  sequence: number | null;
  expiresAt: number | null;
  listings: StoreListing[];
}

export interface StoreStaged {
  stageId: string;
  listingId: string;
  kind: StoreListingKind;
  name: string;
  version: string;
  publisher: string;
  sizeBytes: number;
  sha256: string;
  permissions: StorePermission[];
  budget: StoreBudget | null;
  /** verified: downloaded and matched now. on_download: checked as it downloads after approval. */
  hashCheck: "verified" | "on_download";
  expiresAt: number;
}

export interface StoreInstalled {
  listingId: string;
  kind: StoreListingKind;
  name: string;
  version: string;
  target: string;
  installedAt: number;
  /** paused | active (daemon); not_selected (model on disk, not in use); installing; missing. */
  state: "paused" | "active" | "not_selected" | "installing" | "missing";
}

export interface StoreDomain {
  catalog(): Promise<StoreCatalogView>;
  stage(listingId: string): Promise<StoreOutcome<StoreStaged>>;
  /** The member approved the install sheet for this stage (HIC). Installs PAUSED. */
  install(stageId: string, onProgress?: (pct: number) => void): Promise<StoreOutcome<StoreInstalled>>;
  installed(): Promise<StoreInstalled[]>;
  remove(listingId: string): Promise<StoreOutcome<string>>;
}
