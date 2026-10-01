// HUP-S1.4 — the interview card is reachable from chat ("Plan it first"), and an accepted brief
// renders in the thread as a card.
import { describe, it, expect } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { AgentChat } from "./AgentChat";
import { store } from "../shell/store";
import { freshState } from "../shell/state";

describe("AgentChat — Plan it first", () => {
  it("offers 'Plan it first' next to Send", () => {
    store.setState({ ...freshState("p1"), chatMsgs: [], chatStatus: "ready" });
    const html = renderToStaticMarkup(<AgentChat store={store} s={store.state} />);
    expect(html).toContain('data-testid="plan-it-first"');
    expect(html).toContain("Plan it first");
  });

  it("renders an accepted brief as a card in the thread", () => {
    store.setState({ ...freshState("p1"), chatMsgs: [], chatStatus: "ready" });
    store.acceptBrief(
      { track: "code", goal: "fix the parser", constraints: [], persona: "Builder", skills: [], workflow: "code-change", workflow_available: false, ships_in: "0.5.0", gates: ["tests pass"] },
      "# Brief\n\n**Goal:** fix the parser",
    );
    const html = renderToStaticMarkup(<AgentChat store={store} s={store.state} />);
    expect(html).toContain('data-testid="brief-card"');
    expect(html).toContain("fix the parser");
    expect(html).toContain("nothing is built until the workflow ships");
  });
});
