// L6 bridge impl, SIM (web/dev): `bridge.mcpCatalog` in the browser preview (src/bridge/lanes/l6.ts).
// The preview has no backend for L6, so a member added here throws `Unavailable`
// (src/bridge/types.ts) unless the prototype Store holds an honest answer (Rule 1). No members yet.
import type { McpCatalogDomain } from "../lanes/l6";
import type { SimHost } from "./index";

export function simMcpCatalog(_host: SimHost): McpCatalogDomain {
  return {};
}
