// K3 red-first stub (replaced by the Node › Advanced tab in the next commit). It keeps the old
// behaviour: Node › Advanced is the Node configuration section and nothing else.
import type { ReactNode } from "react";
import type { NodeStatusView as NodeStatusDto } from "../../bridge/domains";
import type { NetworkCompat } from "../../shell/networkCompat";
import type { AppState } from "../../shell/state";

export interface AdvancedReads {
  nodeStatus(): Promise<NodeStatusDto>;
  lastCrash(): Promise<{ reason: string; atUnixMs: number } | null>;
}

export function NodeAdvancedView({ config }: { s: AppState; mode: "tauri" | "sim"; compat: NetworkCompat; liveGenesis: string | null; expectedGenesis: string | null; reads: AdvancedReads; config?: ReactNode }) {
  return <>{config}</>;
}
