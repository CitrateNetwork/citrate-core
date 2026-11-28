// L6 bridge impl, TAURI: `bridge.mcpCatalog` in the desktop app (src/bridge/lanes/l6.ts). Each member
// L6 adds here invokes one registered L6 command (src-tauri/src/commands/l6.rs) through
// `./invoke`. No members yet, so nothing is invoked.
import type { McpCatalogDomain } from "../lanes/l6";

export const tauriMcpCatalog: McpCatalogDomain = {};
