// C1-V #347 review — what a failed `signing.broadcast` means for the member. Shared by the wallet
// review (store) and the dApp provider (ceremonyConnector), so both read the backend the same way.
/**
 * C1-V #347 review — classify a `signing.broadcast` rejection. The backend prefixes every failure
 * established BEFORE the node could have received the tx with "not submitted: "; only those are
 * "not sent". "submitted, outcome unknown (tx <hash>)" carries the hash; any other error (an IPC
 * deadline, a crash) is an unknown outcome without a hash, never "not sent".
 */
export function broadcastFailure(err: unknown): { kind: "not-sent"; reason: string } | { kind: "unknown"; txHash: string | null; reason: string } {
  const msg = typeof err === "string" ? err : err instanceof Error ? err.message : String(err);
  const NOT_SUBMITTED = "not submitted: ";
  if (msg.startsWith(NOT_SUBMITTED)) return { kind: "not-sent", reason: msg.slice(NOT_SUBMITTED.length) };
  const m = /submitted, outcome unknown \(tx (0x[0-9a-fA-F]{64})\):\s*(.*)$/.exec(msg);
  if (m) return { kind: "unknown", txHash: m[1], reason: m[2] || msg };
  return { kind: "unknown", txHash: null, reason: msg };
}

/**
 * C1-V3 — the identity of an agent chain effect (its target and calldata, case-insensitive), so an
 * identical payment proposed after an unknown outcome can name the earlier tx. Null when the
 * sidecar exposed neither.
 */
export function agentEffectKey(to: string | undefined, data: string | undefined): string | null {
  if (!to && !data) return null;
  return (to ?? "").toLowerCase() + "|" + (data ?? "").toLowerCase();
}
