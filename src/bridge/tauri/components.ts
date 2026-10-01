// Bridge impl — signed first-run components (HUP-S5.5 / S6.1), TAURI. Rust `components.rs`:
// status is read-only; update refuses until the component signing key is set at the key
// ceremony, before any network or disk access.
import { invoke } from "./invoke";
import type { ComponentsDomain, ComponentsStatus } from "../domains";

export const tauriComponents: ComponentsDomain = {
  status() {
    return invoke<ComponentsStatus>("components_status");
  },
  update(name: string) {
    return invoke<string>("components_update", { name });
  },
  rollback(name: string) {
    return invoke<string>("components_rollback", { name });
  },
};
