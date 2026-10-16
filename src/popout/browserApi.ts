// =====================================================================
// citrate-core — Hermes's browser through the Rust commands (HUP-S5.1 + S5.6)
//
// Thin wrappers over the main-window-only `hermes_browser_*` commands (src-tauri/src/browser.rs).
// Rust checks every input again before anything reaches the Hermes sidecar.
// =====================================================================
import { invoke } from "../bridge/tauri/invoke";

export interface BrowserApi {
  status(): Promise<unknown>;
  frame(after: number): Promise<unknown>;
  stop(): Promise<void>;
  resume(): Promise<void>;
  attach(port: number, consent: boolean): Promise<void>;
  detach(): Promise<void>;
  origin(origin: string, allow: boolean, includeSensitive: boolean): Promise<string>;
  decide(id: string, allow: boolean): Promise<void>;
}

export const tauriBrowserApi: BrowserApi = {
  status: () => invoke<unknown>("hermes_browser_status"),
  frame: (after) => invoke<unknown>("hermes_browser_frame", { after }),
  stop: () => invoke<void>("hermes_browser_stop"),
  resume: () => invoke<void>("hermes_browser_resume"),
  attach: (port, consent) => invoke<void>("hermes_browser_attach", { port, consent }),
  detach: () => invoke<void>("hermes_browser_detach"),
  origin: (origin, allow, includeSensitive) => invoke<string>("hermes_browser_origin", { origin, allow, includeSensitive }),
  decide: (id, allow) => invoke<void>("hermes_browser_decide", { id, allow }),
};
