// =====================================================================
// citrate-core: v060 lane bridge composition (planset 2026-11-07-v060, L0-WP05,
// lanes/L0_INTEGRATION_SEAMS.md section 3.3).
//
// Each 0.6.0 lane gets one namespace on the bridge, declared in src/bridge/lanes/l<N>.ts and
// implemented in src/bridge/tauri/l<N>.ts and src/bridge/sim/l<N>.ts, files only that lane edits.
// This file (L0) wires them in; ../index.ts spreads `v60Tauri()` / `v60Sim(host)` after the CX
// domains. A namespace with no members is a type, not a stub: nothing calls it until its lane adds
// a member with a real data source (Rule 1, Rule 7).
// =====================================================================
import type { SimHost } from "../sim";

import type { AcpDomain } from "./l1";
import type { HicDomain } from "./l2";
import type { NotifyDomain } from "./l3";
import type { ConversationsDomain } from "./l4";
import type { MediaDomain } from "./l5";
import type { McpCatalogDomain } from "./l6";
import type { NetDomain } from "./l7";
import type { TrainTogetherDomain } from "./l8";
import type { PqcDomain } from "./l9";

import { tauriAcp } from "../tauri/l1";
import { tauriHic } from "../tauri/l2";
import { tauriNotify } from "../tauri/l3";
import { tauriConversations } from "../tauri/l4";
import { tauriMedia } from "../tauri/l5";
import { tauriMcpCatalog } from "../tauri/l6";
import { tauriNet } from "../tauri/l7";
import { tauriTrainTogether } from "../tauri/l8";
import { tauriPqc } from "../tauri/l9";

import { simAcp } from "../sim/l1";
import { simHic } from "../sim/l2";
import { simNotify } from "../sim/l3";
import { simConversations } from "../sim/l4";
import { simMedia } from "../sim/l5";
import { simMcpCatalog } from "../sim/l6";
import { simNet } from "../sim/l7";
import { simTrainTogether } from "../sim/l8";
import { simPqc } from "../sim/l9";

export type { AcpDomain, HicDomain, NotifyDomain, ConversationsDomain, MediaDomain, McpCatalogDomain, NetDomain, TrainTogetherDomain, PqcDomain };

/** Lane -> its bridge key. */
export const V60_LANE_KEYS = {
  L1: "acp",
  L2: "hic",
  L3: "notify",
  L4: "conversations",
  L5: "media",
  L6: "mcpCatalog",
  L7: "net",
  L8: "trainTogether",
  L9: "pqc",
} as const;

/** The lane namespaces. No key may equal a legacy or CX domain (seams/types.test-d.ts). */
export interface V60Bridge {
  acp: AcpDomain;
  hic: HicDomain;
  notify: NotifyDomain;
  conversations: ConversationsDomain;
  media: MediaDomain;
  mcpCatalog: McpCatalogDomain;
  net: NetDomain;
  trainTogether: TrainTogetherDomain;
  pqc: PqcDomain;
}

/** Lane namespaces, tauri (desktop) side. */
export function v60Tauri(): V60Bridge {
  return {
    acp: tauriAcp,
    hic: tauriHic,
    notify: tauriNotify,
    conversations: tauriConversations,
    media: tauriMedia,
    mcpCatalog: tauriMcpCatalog,
    net: tauriNet,
    trainTogether: tauriTrainTogether,
    pqc: tauriPqc,
  };
}

/** Lane namespaces, sim (web/dev) side. */
export function v60Sim(host: SimHost): V60Bridge {
  return {
    acp: simAcp(host),
    hic: simHic(host),
    notify: simNotify(host),
    conversations: simConversations(host),
    media: simMedia(host),
    mcpCatalog: simMcpCatalog(host),
    net: simNet(host),
    trainTogether: simTrainTogether(host),
    pqc: simPqc(host),
  };
}
