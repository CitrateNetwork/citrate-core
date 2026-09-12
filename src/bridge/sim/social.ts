// CX bridge impl — social identity (Connections · social discovery), SIM. Honest-empty (Rule 1).
// There is no OAuth or ceremony in the web preview — linking is a desktop-only, keyring-backed,
// wallet-signed flow (ADR-2026-08-30). So status() is honestly empty and every mutating op reports
// that it needs the desktop app rather than pretending to link.
import type { LinkedIdentity, SocialDomain } from "../domains";
import type { SimHost } from "./index";

export function simSocial(_host: SimHost): SocialDomain {
  const desktopOnly = (): never => {
    throw new Error("Linking a social identity needs the desktop app (OAuth + the OS keyring + your wallet signature).");
  };
  return {
    async status(): Promise<LinkedIdentity[]> {
      return [];
    },
    async start(): Promise<LinkedIdentity> {
      return desktopOnly();
    },
    async verifyRequest() {
      return desktopOnly();
    },
    async verifyApprove(): Promise<LinkedIdentity> {
      return desktopOnly();
    },
    async verifyForget(): Promise<void> {
      /* sim: nothing pending */
    },
    async setVisibility(): Promise<LinkedIdentity> {
      return desktopOnly();
    },
    async disconnect(): Promise<void> {
      desktopOnly();
    },
    async resolve() {
      // sim: no links, no faces (honest-empty; the address fallback renders).
      return [];
    },
    async exportBinding() {
      return null;
    },
    async ingestBinding() {
      return false;
    },
    async directoryPublishRequest() {
      return desktopOnly();
    },
    async directoryPublishApprove(): Promise<LinkedIdentity> {
      return desktopOnly();
    },
    async directoryUnpublishRequest() {
      return desktopOnly();
    },
    async directoryUnpublishApprove(): Promise<LinkedIdentity> {
      return desktopOnly();
    },
    async directoryForget(): Promise<void> {
      /* sim: nothing pending */
    },
    async directoryLookup() {
      // sim: no authority session — honest null (never a fabricated address, D-7 / Rule 1).
      return null;
    },
    async directorySearch() {
      return [];
    },
  };
}
