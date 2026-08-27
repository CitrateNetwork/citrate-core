// CX surface — Models (C-16). Owned by lane s1 (CX-S1) after S0. Shell only.
import { SurfaceProps } from "./shared";
import { CxScaffold } from "./CxScaffold";

export function Models(_props: SurfaceProps) {
  return (
    <CxScaffold
      title="Models"
      sprint="CX-S1"
      detail="Browse, download, and switch models — your local models plus downloadable ones from Hugging Face and GitHub, with one-tap OAuth."
    />
  );
}
