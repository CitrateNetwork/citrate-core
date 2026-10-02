// HUP-S3.3 (US-3.3 AC2) — a saved brief's workflow runs from its card in the chat thread.
import { describe, it, expect } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { AgentChat } from "./AgentChat";
import { store } from "../shell/store";
import { freshState } from "../shell/state";

const BRIEF = { track: "creative", goal: "a poster", constraints: [], persona: "Maker", skills: [], workflow: "creative-project", workflow_available: true, ships_in: null, gates: ["Nothing is published until you review it"] };

describe("AgentChat: run a brief's workflow", () => {
  it("a saved brief's card offers to run its workflow and says how to", () => {
    store.setState({ ...freshState("p1"), chatMsgs: [], chatStatus: "ready" });
    store.acceptBrief(BRIEF as never, "# Brief\n\n**Goal:** a poster");
    const html = renderToStaticMarkup(<AgentChat store={store} s={store.state} />);
    expect(html).toContain('data-testid="brief-run-workflow"');
    expect(html).toContain("Run creative-project");
    expect(html).toContain("run its workflow from this card");
  });

  it("the button is disabled while Hermes is busy", () => {
    store.setState({ ...freshState("p1"), chatMsgs: [], chatStatus: "ready" });
    store.acceptBrief(BRIEF as never, "# Brief");
    store.setState({ chatStatus: "thinking" });
    const html = renderToStaticMarkup(<AgentChat store={store} s={store.state} />);
    expect(html).toMatch(/data-testid="brief-run-workflow" disabled=""|disabled="" data-testid="brief-run-workflow"/);
  });

  it("an ordinary message has no run button", () => {
    store.setState({ ...freshState("p1"), chatMsgs: [{ id: "m1", who: "Agent", text: "hello", chips: [], streaming: false }], chatStatus: "ready" });
    const html = renderToStaticMarkup(<AgentChat store={store} s={store.state} />);
    expect(html).not.toContain("brief-run-workflow");
  });
});
