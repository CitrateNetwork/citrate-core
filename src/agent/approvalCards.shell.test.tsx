// HUP-S2.2 (US-2.2 AC2) — a shell_run approval card shows the exact command (one argument per
// entry, never re-joined), the folder, the program that will run, the timeout and the OS sandbox,
// and the SignatureCeremony renders it with the HIC banner.
import { describe, it, expect } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { shellRunCard, SHELL_RUN_HIC_REASON } from "./approvalCards";
import { SignatureCeremony } from "../shell/Chrome";
import { freshState, type CerSpec } from "../shell/state";
import type { Store } from "../shell/store";
import type { ShellPendingView } from "../bridge/domains";

const pending: ShellPendingView = {
  id: "sh-3",
  callId: "c1",
  tool: "shell_run",
  hic: "required",
  argv: ["bash", "-c", "echo a; rm -rf x"],
  resolvedProgram: "/bin/bash",
  cwd: "/Users/m/proj",
  timeoutSecs: 90,
  expiresInSecs: 300,
  sandbox: {
    backend: "seatbelt",
    enforced: true,
    network: "denied",
    writable: ["/Users/m/proj", "scratch HOME (deleted after the run)"],
    readable_extra: [],
    summary: "macOS Seatbelt: no network; writes only in /Users/m/proj and a scratch HOME",
  },
};

describe("HUP-S2.2 shell_run approval card", () => {
  it("carries the exact argv, folder, program, timeout and sandbox", () => {
    const card = shellRunCard(pending);
    expect(card.kind).toBe("command");
    if (card.kind !== "command") return;
    expect(card.argv).toEqual(["bash", "-c", "echo a; rm -rf x"]);
    expect(card.cwd).toBe("/Users/m/proj");
    expect(card.summary).toContain("bash");
    expect(card.rows).toEqual([
      { k: "Program", v: "/bin/bash" },
      { k: "Timeout", v: "90 s" },
      { k: "Sandbox", v: pending.sandbox.summary },
      { k: "Network", v: "denied" },
      { k: "Can write", v: "/Users/m/proj; scratch HOME (deleted after the run)" },
    ]);
  });

  it("says plainly when the OS sandbox is not enforced", () => {
    const card = shellRunCard({ ...pending, sandbox: { ...pending.sandbox, backend: "none", enforced: false, network: "allowed", summary: "no OS sandbox on this machine" } });
    if (card.kind !== "command") throw new Error("command card expected");
    expect(card.rows?.find((r) => r.k === "Sandbox")?.v).toMatch(/not enforced/i);
  });

  it("renders in the SignatureCeremony with the HIC banner and every argument", () => {
    const s = freshState("p1");
    const head: CerSpec = { origin: "chat agent", requester: "Hermes · tool shell_run", title: "Approve a command", rows: [], cost: "none", sponsor: "explicit decision required", sponsorColor: "var(--tx-3)", chainless: true, card: shellRunCard(pending), hic: { reason: SHELL_RUN_HIC_REASON } };
    s.queue = [head];
    s.cerPhase = "review";
    const html = renderToStaticMarkup(<SignatureCeremony store={{} as unknown as Store} s={s} />);
    expect(html).toContain('data-testid="card-command"');
    expect(html).toContain('data-testid="hic-banner"');
    expect((html.match(/data-testid="argv"/g) ?? []).length).toBe(3);
    expect(html).toContain("/bin/bash");
    expect(html).toContain("macOS Seatbelt");
    expect(SHELL_RUN_HIC_REASON).not.toMatch(/HITL|—/);
  });
});
