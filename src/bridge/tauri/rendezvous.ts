// CX bridge impl — GROW-S2 relay rendezvous (Join 9), TAURI. Real invokes of the rendezvous_*
// commands (src-tauri/src/rendezvous.rs), which speak to the member daemon. Views only.
import { listen } from "@tauri-apps/api/event";
import { invoke } from "./invoke";
import type { JoinViewDto, RendezvousDomain, RendezvousViews } from "../domains";

/** The Rust `rendezvous::JOIN_STATE_EVENT`. */
export const JOIN_STATE_EVENT = "join://state";

export const tauriRendezvous: RendezvousDomain = {
  tick(): Promise<RendezvousViews> {
    return invoke<RendezvousViews>("rendezvous_tick");
  },
  cancel(group: string): Promise<JoinViewDto> {
    return invoke<JoinViewDto>("rendezvous_cancel", { group });
  },
  async forget(group: string): Promise<void> {
    await invoke("rendezvous_forget", { group });
  },
  async onState(fn) {
    return listen<RendezvousViews>(JOIN_STATE_EVENT, (ev) => fn(ev.payload));
  },
};
