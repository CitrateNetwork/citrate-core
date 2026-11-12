// V60 MESH-4: the member's on/off control for the cross-machine mesh. Source: `cluster_mesh_status`
// (`MeshStatus`). A change goes through `cluster_mesh_set_enabled`, which core refuses until the
// CL-S4 transport sign-off is recorded in the build, and takes effect when the cluster next starts.
import type { MeshStatus } from "../bridge/domains";

export interface MeshSwitchModel {
  /** The control can be used. */
  enabled: boolean;
  /** The mesh is on for this app run. */
  on: boolean;
  /** What pressing the control does, or null when it cannot be used. */
  action: "turn-on" | "turn-off" | null;
  /** Plain-language reason shown next to the control. */
  reason: string;
}

export function meshSwitchModel(mesh: MeshStatus | null): MeshSwitchModel {
  if (!mesh) return { enabled: false, on: false, action: null, reason: "Checking the mesh…" };
  if (!mesh.signedOff) {
    return {
      enabled: false,
      on: mesh.on,
      action: null,
      reason: "You can turn the mesh on or off here once its security review is signed off.",
    };
  }
  if (mesh.source === "operator") {
    return {
      enabled: false,
      on: mesh.on,
      action: null,
      reason: "An operator setting on this machine turns the mesh on; it cannot be changed here.",
    };
  }
  return {
    enabled: true,
    on: mesh.on,
    action: mesh.on ? "turn-off" : "turn-on",
    reason: "A change applies the next time the cluster starts.",
  };
}
