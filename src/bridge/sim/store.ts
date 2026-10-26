// Bridge impl — Store v1 (K5a), SIM (web/dev). A browser preview has no catalog verifier, no
// daemon book and no models folder, so it lists nothing and says the Store needs the desktop app
// (Rule 1: no invented listings).
import type { StoreDomain } from "../domains";
import type { SimHost } from "./index";

const DESKTOP_ONLY = "The Store needs the desktop app: this preview cannot verify the signed catalog.";

export function simStore(_host: SimHost): StoreDomain {
  return {
    async catalog() {
      return {
        state: "desktop_only",
        message: DESKTOP_ONLY,
        catalogUrl: "",
        keyFingerprint: null,
        sequence: null,
        expiresAt: null,
        listings: [],
      };
    },
    async stage() {
      return { refused: { code: "desktop_only", message: DESKTOP_ONLY } };
    },
    async install() {
      return { refused: { code: "desktop_only", message: DESKTOP_ONLY } };
    },
    async installed() {
      return [];
    },
    async remove() {
      return { refused: { code: "desktop_only", message: DESKTOP_ONLY } };
    },
  };
}
