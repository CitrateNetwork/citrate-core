// CX surface — Train (C-21). Owned by lane s5 (CX-S5) after S0. Shell only.
import { SurfaceProps } from "./shared";
import { CxScaffold } from "./CxScaffold";

export function Train(_props: SurfaceProps) {
  return (
    <CxScaffold
      title="Train together"
      sprint="CX-S5"
      detail="Your Group trains a model together: each member trains locally and contributes, the network aggregates the result, and contributors are settled in SALT for verified work."
    />
  );
}
