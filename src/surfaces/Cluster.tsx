// CX surface — Cluster (C-20). Owned by lane s4 (CX-S4) after S0. Shell only.
import { SurfaceProps } from "./shared";
import { CxScaffold } from "./CxScaffold";

export function Cluster(_props: SurfaceProps) {
  return (
    <CxScaffold
      title="Cluster"
      sprint="CX-S4"
      detail="Your Group's members form a private, secure peer-to-peer cluster to share files, storage, and compute — membership is authorized by the Group's roster."
    />
  );
}
