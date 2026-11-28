// citrate-core: bridge domain `connections`, moved verbatim from src/bridge/domains.ts by L0-WP05
// (planset v060). src/bridge/domains.ts re-exports it, so imports from "../bridge/domains" keep
// working. The owning lane is the row for this path in .agentile/v60-ownership.map.

/** One MCP connection's state (W4). Mirrors the Rust `ConnectionStatus`
 * (camelCase). NO token ever crosses this boundary — only connect facts. */
export interface ConnectionInfo {
  /** Internal service id: `github` | `gdrive` | `notion`. */
  service: string;
  connected: boolean;
  scope: string | null;
  connectedAt: number | null;
}

/** MCP connections (Google Drive / Notion / GitHub) via OAuth loopback-PKCE.
 * `start` opens the provider's authorize page in the SYSTEM browser and, on
 * success, seals the token in the OS-keyring-backed custody vault — the token
 * is never returned. Desktop-only: the sim preview reports honestly disconnected
 * and throws Unavailable on `start`. */
export interface ConnectionsDomain {
  /** Connect/disconnect state of all three services. */
  status(): Promise<ConnectionInfo[]>;
  /** Run the OAuth flow for one service; resolves with its new status. */
  start(service: string): Promise<ConnectionInfo>;
  /** Forget a service's sealed token. */
  disconnect(service: string): Promise<void>;
}
