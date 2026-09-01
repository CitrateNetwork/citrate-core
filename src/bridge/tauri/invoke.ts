// =====================================================================
// citrate-core — timeout-wrapped Tauri invoke (UI-hang guard)
//
// Every Tauri command in the app goes through THIS `invoke`, not the raw one, so a command that never
// resolves (a wedged daemon socket, a stalled RPC route, a hung sidecar) can never leave a surface
// stuck on an infinite spinner. On timeout the returned promise REJECTS with `InvokeTimeout`, so the
// caller's normal error path fires and the surface falls to its honest empty/error state (Rule 1).
//
// This is a BACKSTOP layered above the Rust-side deadlines (comms/cluster UDS read timeouts, the RPC
// ureq timeout): those bound the work at ~5-6s and return a real error first; this catches anything
// with no Rust-side bound (or a hung invoke bridge itself). The default is set comfortably above the
// Rust deadlines so a real, slightly-slow call still returns its true result/error rather than being
// pre-empted here. NOTE: rejecting does not cancel the underlying Rust work (Tauri has no cancel) —
// it just unblocks the UI; the late result is discarded harmlessly.
// =====================================================================
import { invoke as rawInvoke, type InvokeArgs } from "@tauri-apps/api/core";

/** Default backstop deadline (ms). Above the Rust IPC (5s) / RPC (6s) bounds so those surface their
 *  own honest error first; this only fires for a command with no server-side bound. */
export const INVOKE_TIMEOUT_MS = 12_000;

/** Commands that are LEGITIMATELY long-running and must NOT be pre-empted: browser/loopback flows that
 *  wait on the human (OAuth logins can take minutes), large file work, and first-run bulk writes.
 *  Bounding these would break a real login or a model download. Everything else — the daemon reads,
 *  chain RPC, roster/list/status calls that caused the pinwheel — keeps the default deadline. */
const UNBOUNDED = new Set<string>([
  "auth_login", // OIDC loopback — waits for the user to authenticate in the browser
  "connection_start", // MCP OAuth loopback — waits for the browser
  "social_start", // X/Discord OAuth loopback — waits for the browser
  "kyc_start", // opens the KYC flow in the browser
  "membership_checkout", // opens the external checkout popup
  "model_download", // large, resumable download (drives its own progress)
  "model_verify", // SHA-256 over a multi-GB model file
  "model_serve_start", // spawns the llama-server sidecar
  "memory_ingest_docs", // first-run bulk docs preload
  "memory_seed_context", // first-run bulk memory writes
]);

/** Thrown when a command exceeds its deadline. Distinct type so callers/telemetry can tell a timeout
 *  from a real command error. */
export class InvokeTimeout extends Error {
  constructor(
    public readonly command: string,
    public readonly ms: number,
  ) {
    super(`command "${command}" timed out after ${ms}ms`);
    this.name = "InvokeTimeout";
  }
}

/** Drop-in replacement for `@tauri-apps/api/core`'s `invoke`, with a hard deadline. Pass a custom
 *  `timeoutMs` for a command that is legitimately long-running (e.g. a model download drives its own
 *  progress and should not be bounded here). */
export function invoke<T>(command: string, args?: InvokeArgs, timeoutMs: number = INVOKE_TIMEOUT_MS): Promise<T> {
  // Preserve call arity: forward the no-args form as `invoke(cmd)` (not `invoke(cmd, undefined)`), so
  // the underlying call is byte-identical to a direct invoke (and the contract tests still match).
  const call = args === undefined ? rawInvoke<T>(command) : rawInvoke<T>(command, args);
  // Long-running-by-design commands run unbounded (a deadline here would break a real login/download).
  if (UNBOUNDED.has(command) || !Number.isFinite(timeoutMs)) return call;
  let timer: ReturnType<typeof setTimeout> | undefined;
  const deadline = new Promise<never>((_resolve, reject) => {
    timer = setTimeout(() => reject(new InvokeTimeout(command, timeoutMs)), timeoutMs);
  });
  return Promise.race([call, deadline]).finally(() => {
    if (timer !== undefined) clearTimeout(timer);
  }) as Promise<T>;
}
