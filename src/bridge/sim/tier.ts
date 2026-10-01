// Bridge impl — tier (HUP-S1.6), SIM (web/dev). A browser preview cannot read this machine's
// hardware, so it reports NO tier rather than an invented one (Rule 1), and choosing a tier needs
// the desktop app.
import type { TierDomain } from "../domains";
import type { SimHost } from "./index";

export function simTier(_host: SimHost): TierDomain {
  return {
    async recommend() {
      return null;
    },
    async setOverride() {
      throw new Error("choosing a hardware tier needs the desktop app");
    },
  };
}
