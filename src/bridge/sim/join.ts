// CX bridge impl — v2 invite links (GROW-S1), SIM. The web preview has no join client, keyring or
// relay, so every action says so (Rule 1) and nothing is ever waiting.
import type { JoinCodeEntry, JoinDomain, JoinMinted, JoinQr, JoinResolved, PendingJoinView } from "../domains";
import type { SimHost } from "./index";

const DESKTOP = "invite links need the desktop app";

export function simJoin(_host: SimHost): JoinDomain {
  return {
    async mint(): Promise<JoinMinted> {
      throw new Error(DESKTOP);
    },
    async attach(): Promise<JoinMinted> {
      throw new Error(DESKTOP);
    },
    async redeem(): Promise<void> {
      throw new Error(DESKTOP);
    },
    async revoke(): Promise<void> {
      throw new Error(DESKTOP);
    },
    async list(): Promise<JoinCodeEntry[]> {
      return [];
    },
    async pendingSet(): Promise<PendingJoinView> {
      throw new Error(DESKTOP);
    },
    async pendingGet(): Promise<PendingJoinView | null> {
      return null;
    },
    async pendingClear(): Promise<void> {
      /* nothing is ever held in the web preview */
    },
    async pendingResolve(): Promise<JoinResolved> {
      throw new Error(DESKTOP);
    },
    async shareQr(): Promise<JoinQr> {
      throw new Error(DESKTOP);
    },
  };
}
