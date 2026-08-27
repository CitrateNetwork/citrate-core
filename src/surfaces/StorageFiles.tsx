// CX surface — StorageFiles (C-17). Owned by lane s2 (CX-S2) after S0. Shell only.
import { SurfaceProps } from "./shared";
import { CxScaffold } from "./CxScaffold";

export function StorageFiles(_props: SurfaceProps) {
  return (
    <CxScaffold
      title="Files"
      sprint="CX-S2"
      detail="Drag and drop files to store and pin them on the network, list what you hold, and retrieve by CID. The network rewards pinners who keep data available."
    />
  );
}
