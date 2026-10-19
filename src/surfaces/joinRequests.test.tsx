// GROW-S2 (Join 9, M5) — the join rows and invite state line render the §11 words and actions.
import { describe, it, expect } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { JoinRequests, InviteStateLine } from "./JoinRequests";
import type { InviteViewDto, JoinViewDto } from "../bridge/domains";

const noop = () => {};
const join = (state: JoinViewDto["state"], over: Partial<JoinViewDto> = {}): JoinViewDto => ({
  group: "g1",
  name: "Aperture",
  state,
  reason: null,
  expiresAt: null,
  graceUntil: null,
  updatedAt: 1,
  ...over,
});

describe("JoinRequests", () => {
  it("renders nothing with no joins", () => {
    expect(renderToStaticMarkup(<JoinRequests joins={[]} lastKnown={{}} listedGroups={[]} busy={null} onRetry={noop} onCancel={noop} onForget={noop} />)).toBe("");
  });

  it("shows the waiting words with Cancel, and Joined only once the group is listed", () => {
    const html = renderToStaticMarkup(
      <JoinRequests joins={[join("waiting"), join("joined", { group: "g2" })]} lastKnown={{}} listedGroups={["g2"]} busy={null} onRetry={noop} onCancel={noop} onForget={noop} />,
    );
    expect(html).toContain("Waiting for the person who invited you to let you in");
    expect(html).toContain("Cancel");
    expect(html).toContain("Joined Aperture");
    const unlisted = renderToStaticMarkup(
      <JoinRequests joins={[join("joined")]} lastKnown={{}} listedGroups={[]} busy={null} onRetry={noop} onCancel={noop} onForget={noop} />,
    );
    expect(unlisted).not.toContain("Joined Aperture");
  });

  it("relay_unreachable renders Retry and the last known state, never expired", () => {
    const html = renderToStaticMarkup(
      <JoinRequests joins={[join("relay_unreachable")]} lastKnown={{ g1: join("waiting") }} listedGroups={[]} busy={null} onRetry={noop} onCancel={noop} onForget={noop} />,
    );
    expect(html).toContain("Couldn&#x27;t check your invite · relay unreachable");
    expect(html).toContain("last known: Waiting…");
    expect(html).toContain("Retry");
    expect(html.toLowerCase()).not.toContain("expired");
  });
});

describe("InviteStateLine", () => {
  const view: InviteViewDto = { lookup: "l", group: "g", state: "welcome_expired", joiner: "0x12aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaab", expiresAt: 1, updatedAt: 1, reason: null, dropped: 0 };
  it("offers Remove on welcome_expired", () => {
    const html = renderToStaticMarkup(<InviteStateLine view={view} lastKnown={undefined} onRetry={noop} onRemove={noop} />);
    expect(html).toContain("Not picked up · Remove 0x12…ab?");
    expect(html).toContain(">Remove<");
  });
  it("renders nothing before the daemon reports the rendezvous", () => {
    expect(renderToStaticMarkup(<InviteStateLine view={undefined} lastKnown={undefined} onRetry={noop} onRemove={noop} />)).toBe("");
  });
});
