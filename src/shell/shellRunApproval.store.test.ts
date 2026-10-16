// HUP-S2.2 (US-2.2 AC2) — a held shell_run reaches the member on the existing ceremony queue with
// its command card and the HIC banner; only an explicit Approve yields true.
import { describe, it, expect, vi, afterEach } from "vitest";
import { store } from "./store";
import type { ShellPendingView } from "../bridge/domains";

const pending: ShellPendingView = {
  id: "sh-9",
  callId: "c1",
  tool: "shell_run",
  hic: "required",
  argv: ["npm", "test"],
  resolvedProgram: "/opt/homebrew/bin/npm",
  cwd: "/Users/m/app",
  timeoutSecs: 60,
  expiresInSecs: 300,
  sandbox: { backend: "seatbelt", enforced: true, network: "denied", writable: ["/Users/m/app"], readable_extra: [], summary: "macOS Seatbelt: no network" },
};

afterEach(() => {
  vi.restoreAllMocks();
  store.setState({ queue: [] });
});

describe("store.approveShellRun", () => {
  it("queues the command card with the HIC banner and resolves true only on Approve", async () => {
    const sig = vi.spyOn(store, "requestSig").mockResolvedValueOnce("approved").mockResolvedValueOnce("declined");
    expect(await store.approveShellRun(pending)).toBe(true);
    expect(await store.approveShellRun(pending)).toBe(false);
    const spec = sig.mock.calls[0][0];
    expect(spec.card?.kind).toBe("command");
    if (spec.card?.kind === "command") {
      expect(spec.card.argv).toEqual(["npm", "test"]);
      expect(spec.card.cwd).toBe("/Users/m/app");
    }
    expect(spec.hic?.reason).toMatch(/explicit decision/);
    expect(spec.chainless).toBe(true);
  });
});
