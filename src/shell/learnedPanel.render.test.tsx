// HUP-S3.4 — the "What Hermes learned" panel and the proposal card (static renders; no clicks fire).
import { describe, it, expect } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { LearnedPanelView, type LearnedView } from "./LearnedPanel";
import { LearnProposalCard } from "./LearnProposalCard";
import type { LearnedMemory, LearnProposal } from "../agent/learn";
import { TeachHermesView } from "./TeachHermesCard";

const noop = () => undefined;
const SKILL = "---\nname: deploy-checklist\ndescription: Checks a contract before deploy\n---\n\n1. Run the tests.\n";

function p(over: Partial<LearnProposal> = {}): LearnProposal {
  return {
    id: "lp-0123456789abcdef01234567",
    kind: "memory",
    content: { kind: "memory", key: "deploy chain", value: "40204" },
    content_sha256: "ab".repeat(32),
    evidence: {
      workflow_id: "check",
      steps: ["answer"],
      verdicts: [{ step: "answer", name: "tests pass", passed: true, detail: "" }],
      attempts: 2,
      trajectory: { session_id: "s1-ab", workflow_id: "check", messages: 4, sha256: "cd".repeat(32) },
    },
    provenance: { session_id: "s1-ab", agent: "hermes", model: "gemma-4" },
    created_at_ms: 1,
    conflicts: [],
    state: { state: "proposed" },
    ...over,
  };
}

function view(over: Partial<LearnedView> = {}): LearnedView {
  return {
    status: { sidecar: { enabled: true, pending: 1 }, publish: { enabled: false, note: "Publishing learned skills to the on-chain SkillRegistry is not turned on yet (pending owner sign-off)." } },
    proposals: [],
    memories: [],
    error: null,
    ...over,
  };
}

const render = (v: LearnedView) =>
  renderToStaticMarkup(<LearnedPanelView view={v} acked={{}} busy={false} onAck={noop} onAccept={noop} onReject={noop} onPublish={noop} onStorePending={noop} />);

describe("Feature: the proposal card", () => {
  it("Given a proposal, then it shows the evidence and Accept and Reject", () => {
    const html = renderToStaticMarkup(<LearnProposalCard proposal={p()} publish={null} acked={new Set()} onAck={noop} onAccept={noop} onReject={noop} />);
    expect(html).toContain('data-testid="learn-evidence"');
    expect(html).toMatch(/data-testid="learn-verdict" data-pass="true"/);
    expect(html).toContain("2 judged attempts");
    expect(html).toContain("session s1-ab");
    expect(html).toMatch(/data-testid="learn-accept"[^>]*>Accept</);
    expect(html).not.toMatch(/data-testid="learn-accept"[^>]*disabled/);
    expect(html).toContain('data-testid="learn-reject"');
    expect(html).not.toContain("learn-publish");
  });

  it("Given a contradiction, then it shows an acknowledge box and Accept is disabled until ticked", () => {
    const c = p({ conflicts: [{ kind: "contradiction", existing_id: "memory:lm-1", detail: "the known memory lm-1 says something different", blocking: false }] });
    const off = renderToStaticMarkup(<LearnProposalCard proposal={c} publish={null} acked={new Set()} onAck={noop} onAccept={noop} onReject={noop} />);
    expect(off).toMatch(/data-testid="learn-conflict" data-blocking="false"/);
    expect(off).toContain('type="checkbox"');
    expect(off).toMatch(/data-testid="learn-accept"[^>]*disabled/);
    expect(off).toMatch(/Acknowledge each conflict/);
    const on = renderToStaticMarkup(<LearnProposalCard proposal={c} publish={null} acked={new Set(["memory:lm-1"])} onAck={noop} onAccept={noop} onReject={noop} />);
    expect(on).not.toMatch(/data-testid="learn-accept"[^>]*disabled/);
  });

  it("Given a saved skill, then publishing is shown disabled with the honest note", () => {
    const s = p({ kind: "skill", content: { kind: "skill", skill_md: SKILL }, state: { state: "persisted" } });
    const html = renderToStaticMarkup(<LearnProposalCard proposal={s} publish={{ enabled: false, note: "not turned on yet (pending owner sign-off)" }} acked={new Set()} onAck={noop} onAccept={noop} onReject={noop} onPublish={noop} />);
    expect(html).toContain('data-testid="learn-publish"');
    expect(html).toMatch(/<button[^>]*disabled[^>]*>Publish to the SkillRegistry/);
    expect(html).toContain("pending owner sign-off");
    expect(html).not.toContain('data-testid="learn-accept"');
  });
});

describe("Feature: the What Hermes learned panel", () => {
  it("Given learning is off, then the panel says so and shows no proposals", () => {
    const html = render(view({ status: { sidecar: { enabled: false, error: "Hermes is not running" }, publish: { enabled: false, note: "" } } }));
    expect(html).toContain('data-testid="learned-off"');
    expect(html).toContain("Hermes is not running");
    expect(html).not.toContain("learn-card");
  });

  it("Given no proposals, then the panel explains that only verified work is proposed", () => {
    const html = render(view());
    expect(html).toContain('data-testid="learned-empty"');
    expect(html).toMatch(/workflow whose checks all passed/);
  });

  it("Given waiting proposals, saved skills and memories, then each is shown in its place", () => {
    const html = render(
      view({
        proposals: [p(), p({ id: "lp-0123456789abcdef01234568", kind: "skill", content: { kind: "skill", skill_md: SKILL }, state: { state: "persisted" } }), p({ id: "lp-0123456789abcdef01234569", state: { state: "rejected", by: "m", reason: "" } })],
        memories: [
          { proposalId: "lp-1", key: "deploy chain", value: "40204", belnap: "both", contradicts: ["lp-0"], contentSha256: "ab", workflowId: "check", acceptedBy: "0xm", acceptedAtMs: 1, decisionSeq: 3, graph: { state: "pending" } },
        ],
      }),
    );
    expect(html.match(/data-testid="learn-card"/g)?.length).toBe(2);
    expect(html).toContain("Saved skills");
    expect(html).toMatch(/data-testid="learned-memory" data-belnap="both"/);
    expect(html).toContain("Contradiction, unresolved");
    expect(html).toContain("Store waiting memories");
    expect(html).not.toContain("Rejected");
  });
});

// ---- teach Hermes, and resolving a contradiction ----------------------------------------------


const teach = (over: Partial<Parameters<typeof TeachHermesView>[0]> = {}) =>
  renderToStaticMarkup(
    <TeachHermesView unavailable={null} task="" checks="" running={false} progress={null} invalid="Write the task for Hermes." onTask={noop} onChecks={noop} onRun={noop} {...over} />,
  );

describe("Feature: teach Hermes from the app", () => {
  it("Given nothing typed yet, then the form shows and Run is disabled without nagging", () => {
    const html = teach();
    expect(html).toContain('data-testid="teach-task"');
    expect(html).toContain('data-testid="teach-checks"');
    expect(html).toMatch(/data-testid="teach-run"[^>]*disabled/);
    expect(html).not.toContain("teach-invalid");
  });

  it("Given a task and checks, then Run is enabled", () => {
    const html = teach({ task: "What chain id?", checks: "40204", invalid: null });
    expect(html).not.toMatch(/data-testid="teach-run"[^>]*disabled/);
  });

  it("Given Hermes is not running, then it says why instead of the form", () => {
    const html = teach({ unavailable: "Teaching needs Hermes running on your local model." });
    expect(html).toContain('data-testid="teach-unavailable"');
    expect(html).not.toContain("teach-task");
  });

  it("Given a finished run, then each check's verdict and the outcome are shown", () => {
    const html = teach({
      invalid: null,
      progress: { phase: "unverified", reason: "step task: a check did not pass", verdicts: [{ label: "task: answer mentions 40204", passed: false }] },
    });
    expect(html).toContain('data-phase="unverified"');
    expect(html).toMatch(/data-testid="teach-verdict" data-pass="false"/);
    expect(html).toContain("nothing can be learned from this run");
  });
});

function mem(id: string, value: string, over: Partial<LearnedMemory> = {}): LearnedMemory {
  return {
    proposalId: id,
    key: "deploy chain",
    value,
    belnap: "both",
    contradicts: [],
    contentSha256: "ab",
    workflowId: "check",
    acceptedBy: "0xm",
    acceptedAtMs: 1,
    decisionSeq: 1,
    graph: { state: "stored", nodeId: "0a1b2c3d4e5f" },
    ...over,
  };
}

describe("Feature: resolving a contradiction in the panel", () => {
  const A = "lp-000000000000000000000001";
  const B = "lp-000000000000000000000002";
  const both = [mem(A, "1", { contradicts: [B] }), mem(B, "40204", { contradicts: [A] })];
  const renderMems = (memories: LearnedMemory[], confirming: string | null = null) =>
    renderToStaticMarkup(
      <LearnedPanelView
        view={view({ memories })}
        acked={{}}
        busy={false}
        onAck={noop}
        onAccept={noop}
        onReject={noop}
        onPublish={noop}
        onStorePending={noop}
        confirming={confirming}
        onKeep={noop}
        onConfirmKeep={noop}
        onCancelKeep={noop}
      />,
    );

  it("Given two contradicting memories, then each offers Keep this one", () => {
    const html = renderMems(both);
    expect(html.match(/data-testid="learned-keep"/g)?.length).toBe(2);
    expect(html).not.toContain("learned-keep-confirm");
  });

  it("Given the member pressed Keep this one, then it asks to confirm and names what is set aside", () => {
    const html = renderMems(both, B);
    expect(html).toContain('data-testid="learned-keep-confirm"');
    expect(html).toContain("Keep &quot;40204&quot;");
    expect(html).toContain("set aside &quot;1&quot;");
    expect(html).toContain('data-testid="learned-keep-yes"');
  });

  it("Given a resolved pair, then the set-aside memory says so and nothing offers to resolve", () => {
    const html = renderMems([mem(A, "1", { belnap: "false", retractedFor: B }), mem(B, "40204", { belnap: "true" })]);
    expect(html).toContain('data-belnap="false"');
    expect(html).toContain("Set aside: you kept &quot;40204&quot; instead");
    expect(html).not.toContain("learned-keep");
  });

  it("Given learning is off, then a contradiction is shown but cannot be resolved from here", () => {
    const off = renderToStaticMarkup(
      <LearnedPanelView view={view({ memories: both, status: { sidecar: { enabled: false }, publish: { enabled: false, note: "" } } })} acked={{}} busy={false} onAck={noop} onAccept={noop} onReject={noop} onPublish={noop} onStorePending={noop} onKeep={noop} />,
    );
    expect(off).toContain('data-belnap="both"');
    expect(off).not.toContain("learned-keep");
  });
});
