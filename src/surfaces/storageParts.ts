// =====================================================================
// citrate-core — K4 storage components, in one place for Node › Storage.
//
// The five-place IA (K0/K3) composes Node › Storage from these. Each is a self-contained surface
// taking `SurfaceProps`; none depends on another, so they can be laid out as tabs or stacked.
// Today they are also routed as the Files hub tabs (files · storage · pins · backup).
//
//   Files   — the IPFS file store: add, retrieve, local pin/unpin       (route "files")
//   Memory  — the mem-mcp graph: constellation, search, MCP endpoint    (route "storage")
//   Pins    — the local pin index ("Local pin · no bond") + V3 params   (route "pins")
//   Backup  — verified export / restore of Files and Memory             (route "backup")
// =====================================================================
export { StorageFiles } from "./StorageFiles";
export { Storage as StorageMemory } from "./Storage";
export { StoragePins, PinBondParams } from "./StoragePins";
export { StorageBackup } from "./StorageBackup";
export { PIN_BONDS_NOT_LIVE, LOCAL_PIN_LABEL, pinStateLabel, formatSaltWei } from "./storageTruth";

/** The storage tabs in order, with the existing route each one renders on. */
export const STORAGE_TABS = [
  { id: "files", label: "Files" },
  { id: "storage", label: "Memory" },
  { id: "pins", label: "Pins" },
  { id: "backup", label: "Backup" },
] as const;
