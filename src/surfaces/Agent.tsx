// CX surface — Agent (C-22). Owned by lane s6 (CX-S6) after S0. Shell only.
import { SurfaceProps } from "./shared";
import { CxScaffold } from "./CxScaffold";

export function Agent(_props: SurfaceProps) {
  return (
    <CxScaffold
      title="Agent"
      sprint="CX-S6"
      detail="Bring your own Hermes agent into the node to run skills, code, and communications. It works within your approval — every on-chain action still asks you to sign."
    />
  );
}
