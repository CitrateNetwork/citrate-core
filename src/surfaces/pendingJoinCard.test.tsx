// GROW-S1 task 12 — the waiting-invite card (onboarding + Groups), one static render per state.
// Never a silent join: the only Join control is an explicit "Join <cluster>" button on a resolved,
// actionable invite, and only once setup allows it.
import { describe, it, expect } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { PendingJoinCardView, InvitePasteField } from "./PendingJoinCard";
import type { PendingJoinUi } from "../shell/state";

const noop = () => {};
const h = { onReview: noop, onJoin: noop, onCancel: noop };
const render = (pj: PendingJoinUi, canJoin = true) => renderToStaticMarkup(<PendingJoinCardView pj={pj} canJoin={canJoin} {...h} />);

const waiting: PendingJoinUi = { phase: "waiting", hasKey: true, persisted: true };
const ready: PendingJoinUi = {
  phase: "ready",
  hasKey: true,
  persisted: true,
  resolved: { kind: "direct", clusterName: "Garden club", goal: "storage", inviter: "@dana", inviterVerified: true, exp: "2100-01-01T00:00:00.000Z", actionable: true, keyMissing: false },
};

describe("PendingJoinCardView", () => {
  it("during setup it says the join comes after setup, with no Review or Join", () => {
    const html = render(waiting, false);
    expect(html).toContain("once setup finishes");
    expect(html).not.toContain('data-testid="pending-join-review"');
    expect(html).not.toContain('data-testid="pending-join-join"');
    expect(html).toContain('data-testid="pending-join-cancel"');
  });

  it("once set up, a waiting invite offers Review (not Join)", () => {
    const html = render(waiting, true);
    expect(html).toContain('data-testid="pending-join-review"');
    expect(html).not.toContain('data-testid="pending-join-join"');
  });

  it("a memory-only hold says it won't survive a restart, and that an earlier invite may show instead", () => {
    const html = render({ ...waiting, persisted: false }, false);
    expect(html).toMatch(/restart/i);
    expect(html).toMatch(/earlier invite may show/i);
  });

  it("a resolved direct invite names the cluster and inviter on one explicit Join button", () => {
    const html = render(ready);
    expect(html).toContain("@dana");
    expect(html).toContain("Join Garden club");
    expect(html).toContain('data-testid="pending-join-join"');
    expect(html).toMatch(/verified/i);
  });

  it("a failed Join shows why and keeps the Join button for a retry", () => {
    const html = render({ ...ready, error: "Couldn't join yet: comms daemon not running You can try again." });
    expect(html).toContain("comms daemon not running");
    expect(html).toContain('data-testid="pending-join-join"');
  });

  it("an unverified inviter is labelled", () => {
    const html = render({ ...ready, resolved: { ...ready.resolved!, inviterVerified: false } });
    expect(html).toMatch(/unverified/i);
  });

  it("a resolved invite still waits for setup before Join", () => {
    const html = render(ready, false);
    expect(html).not.toContain('data-testid="pending-join-join"');
    expect(html).toContain("once setup finishes");
  });

  it("a link missing its key says so and offers no Join", () => {
    const html = render({ ...ready, hasKey: false, resolved: { ...ready.resolved!, actionable: false, keyMissing: true } });
    expect(html).toMatch(/missing its key/i);
    expect(html).not.toContain('data-testid="pending-join-join"');
  });

  it("a public link shows who shared it and that it can't add you to a group", () => {
    const html = render({ ...ready, resolved: { ...ready.resolved!, kind: "referral", actionable: false } });
    expect(html).toMatch(/direct invite/i);
    expect(html).not.toContain('data-testid="pending-join-join"');
  });

  it("checking, joining and error states are honest", () => {
    expect(render({ ...waiting, phase: "resolving" })).toMatch(/Checking your invite/);
    expect(render({ ...ready, phase: "joining" })).toMatch(/Joining/);
    const err = render({ ...waiting, phase: "error", error: "That invite has expired or was revoked. Ask for a new one." });
    expect(err).toContain("expired or was revoked");
  });
});

describe("PendingJoinCardView: a Review that failed", () => {
  const failed: PendingJoinUi = { ...waiting, phase: "error", error: "Short invite links are not available right now. Try again later." };

  it("offers Try again (and Dismiss) when the failure is temporary", () => {
    const html = render({ ...failed, retryable: true });
    expect(html).toContain('data-testid="pending-join-retry"');
    expect(html).toContain('data-testid="pending-join-cancel"');
  });

  it("offers only Dismiss when the invite can't be used", () => {
    const html = render({ ...failed, error: "That invite has expired or was revoked. Ask for a new one." });
    expect(html).not.toContain('data-testid="pending-join-retry"');
    expect(html).toContain('data-testid="pending-join-cancel"');
  });
});

describe("InvitePasteField", () => {
  it("is labelled for a link or a code", () => {
    const html = renderToStaticMarkup(<InvitePasteField onSubmit={noop} />);
    expect(html).toContain("Paste invite link or code");
    expect(html).toContain('data-testid="invite-paste-input"');
  });
});
