// L1 bridge impl, TAURI: `bridge.acp` in the desktop app (src/bridge/lanes/l1.ts). Each member
// L1 adds here invokes one registered L1 command (src-tauri/src/commands/l1.rs) through
// `./invoke`.
import type { AcpDomain, AgentHomeStatus } from "../lanes/l1";
import { invoke } from "./invoke";

export const tauriAcp: AcpDomain = {
  authStatus(id) {
    return invoke<AgentHomeStatus>("agent_runtime_auth_status", { id });
  },
  signOut(id) {
    return invoke<AgentHomeStatus>("agent_runtime_sign_out", { id });
  },
  removeAgent(id) {
    return invoke<boolean>("agent_runtime_remove", { id });
  },
};
