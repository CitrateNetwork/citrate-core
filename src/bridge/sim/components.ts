// Bridge impl — signed first-run components (HUP-S5.5 / S6.1), SIM (web/dev). A browser preview
// has no component store, so it reports none (Rule 1) and updates need the desktop app.
import type { ComponentsDomain } from "../domains";
import type { SimHost } from "./index";

export function simComponents(_host: SimHost): ComponentsDomain {
  return {
    async status() {
      return null;
    },
    async update() {
      throw new Error("component updates need the desktop app");
    },
    async rollback() {
      throw new Error("component updates need the desktop app");
    },
  };
}
