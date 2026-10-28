// Bridge impl — cloud model providers (D-11), SIM (web/dev). A browser preview has no OS keychain,
// no model proxy and no usage log, so every operation says so (Rule 1) instead of drawing
// providers, keys or spend.
import type { ProvidersDomain } from "../domains";
import type { SimHost } from "./index";
import { Unavailable } from "../types";

const WHY = "cloud providers run in the desktop app (keys stay in its OS keychain)";

export function simProviders(_host: SimHost): ProvidersDomain {
  const no = (op: string) => async (): Promise<never> => {
    throw new Unavailable("providers", op, WHY);
  };
  return {
    status: no("status"),
    usageSummary: no("usageSummary"),
    setCaps: no("setCaps"),
    setPrice: no("setPrice"),
    halt: no("halt"),
    resume: no("resume"),
    setKey: no("setKey"),
    testKey: no("testKey"),
    removeKey: no("removeKey"),
    addCustom: no("addCustom"),
    confirmCustom: no("confirmCustom"),
    removeCustom: no("removeCustom"),
    resetUsageLog: no("resetUsageLog"),
    preferredGet: no("preferredGet"),
    preferredSet: no("preferredSet"),
  };
}
