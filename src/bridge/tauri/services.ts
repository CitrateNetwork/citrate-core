// Bridge impl — the services registry snapshot (K1a-2), TAURI. Rust `services.rs`
// `services_snapshot`: each row read from that service's kit supervisor; read-only.
import { invoke } from "./invoke";
import type { ServicesDomain, ServicesSnapshot } from "../domains";

export const tauriServices: ServicesDomain = {
  snapshot() {
    return invoke<ServicesSnapshot>("services_snapshot");
  },
};
