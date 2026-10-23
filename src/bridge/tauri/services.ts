// K3 — Node › Services: the three existing Rust commands the bridge did not wrap yet. Data sources
// (Rule 7): `model_serve_status` (serve.rs ServeStatus), `ipfs_status` (ipfs.rs, a bool), and
// `ipfs_stop` (ipfs.rs). Tauri only; the Services tab shows its unavailable state in the web preview
// and never calls these there.
import { invoke } from "./invoke";

export const tauriServiceCommands = {
  /** llama-server supervisor state: "stopped" | "starting" | "running" | "restarting" | "failed". */
  serveStatus(): Promise<{ state: string; healthy: boolean }> {
    return invoke<{ state: string; healthy: boolean }>("model_serve_status");
  },
  /** Whether the kubo supervisor reports Running. */
  ipfsStatus(): Promise<boolean> {
    return invoke<boolean>("ipfs_status");
  },
  /** Release the kubo supervisor. */
  async ipfsStop(): Promise<void> {
    await invoke("ipfs_stop");
  },
};
