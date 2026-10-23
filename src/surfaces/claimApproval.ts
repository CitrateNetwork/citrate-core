// What the owner's app does after approving a claim (Groups → Requests, manual claim paste).
// Data: bridge.invites.verifyConsume (group_invite_verify_consume).
import type { ConsumeOutcome } from "../bridge/domains";

export interface ApprovalStep {
  /** Add the claimant to the group now. */
  add: boolean;
  toast: string;
}

export function approvalStep(outcome: ConsumeOutcome): ApprovalStep {
  if (outcome === "invalid") return { add: false, toast: "That request's invite is invalid or already used." };
  // The invite's relay rendezvous isn't confirmed closed yet: the app adds them once it is.
  if (outcome === "pending") return { add: false, toast: "Approved. They'll be added once the relay confirms the invite is closed." };
  return { add: true, toast: "Request approved — adding them to the group." };
}
