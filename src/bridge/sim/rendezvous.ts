// CX bridge impl — GROW-S2 relay rendezvous (Join 9), SIM. The web preview has no member daemon and
// no relay, so it has no joins and no invites: honest-empty, never a fabricated state (Rule 1).
import type { JoinViewDto, RendezvousDomain, RendezvousViews } from "../domains";
import type { SimHost } from "./index";

export function simRendezvous(_host: SimHost): RendezvousDomain {
  return {
    async tick(): Promise<RendezvousViews> {
      return { joins: [], invites: [] };
    },
    async cancel(): Promise<JoinViewDto> {
      throw new Error("invite joins need the desktop app");
    },
    async forget(): Promise<void> {
      /* sim: nothing to forget */
    },
    async onState() {
      return () => {};
    },
  };
}
