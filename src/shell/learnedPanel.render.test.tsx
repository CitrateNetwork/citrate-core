// HUP-S3.4 — the "What Hermes learned" panel and the proposal card (static renders; no clicks fire).
import { describe, it, expect } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { LearnedPanelView, type LearnedView } from "./LearnedPanel";
import { LearnProposalCard } from "./LearnProposalCard";
import type { LearnProposal } from "../agent/learn";

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
