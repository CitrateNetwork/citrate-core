// FL design board 01 (Hermes › Chat): one test per state in the StateMatrix row for Hermes chat,
// plus the keyboard and accessibility rules. Fixture runs here exist only in this test.
import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { AgentChat, STARTERS } from "./AgentChat";
import { store } from "../shell/store";
import { freshState, type ChatMsg } from "../shell/state";
import { newRun, runEnd, runThink, runToolEnd, runToolStart, runDraft } from "../agent/runTimeline";
import { setEnvelopeForAuthority } from "../fl/authority";
import * as agentSlice from "../shell/slices/agent";

(globalThis as unknown as { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const BANNED = /\b(earn|earning|earnings|income|yield|APY|APR|profit|HITL)\b|Deployed · dormant|—(?!\s\()/i;

function liveRun() {
  let r = runThink(newRun(0), 0, 1);
  r = runToolStart(r, "c0", "node_status", 10, "core");
  r = runToolEnd(r, "c0", "node_status", "ok", '{"height":1204311}', 420);
  return runToolStart(r, "c1", "fl_round_plan", 500, "core");
}
function doneRun() {
  let r = liveRun();
  r = runToolEnd(r, "c1", "fl_round_plan", "ok", "no rounds", 3300);
  r = runDraft(r, 3400);
  return runEnd(r, "answered", 6000);
}
const you = (text: string): ChatMsg => ({ id: "u1", who: "You", text, chips: [], streaming: false });
const agent = (patch: Partial<ChatMsg>): ChatMsg => ({ id: "a1", who: "Agent", text: "", chips: [], streaming: false, ...patch });

describe("board 01 Hermes chat", () => {
  let host: HTMLDivElement;
  let root: Root;
  beforeEach(() => {
    host = document.createElement("div");
    document.body.appendChild(host);
    root = createRoot(host);
    store.setState({ ...freshState("p1"), chatMsgs: [], chatStatus: "ready", chatQueue: [], chatHermesDown: null });
  });
  afterEach(() => {
    act(() => root.unmount());
    host.remove();
    setEnvelopeForAuthority(null);
    vi.restoreAllMocks();
  });
  const render = () => act(() => root.render(<AgentChat store={store} s={store.state} />));
  const q = (id: string) => host.querySelector(`[data-testid="${id}"]`) as HTMLElement | null;
  const qa = (id: string) => Array.from(host.querySelectorAll(`[data-testid="${id}"]`)) as HTMLElement[];

  it("Empty: new chat starters, with the GPU starter and no banned words", () => {
    render();
    expect(q("chat-empty")?.textContent).toContain("What should Hermes do on this machine?");
    const starters = qa("chat-starter").map((b) => b.textContent ?? "");
    expect(starters[0]).toContain("Put my GPU to work on rounds at night.");
    expect(starters).toHaveLength(STARTERS.length);
    expect(host.textContent ?? "").not.toMatch(BANNED);
  });

  it("Running: the live timeline with Stop · Esc, and Esc stops the run", () => {
    const stop = vi.spyOn(store, "stopAgentTurn").mockImplementation(() => undefined);
    store.setState({ chatStatus: "tool", chatMsgs: [you("Can this machine join group rounds?"), agent({ streaming: true, run: liveRun(), model: "Local 8B" })] });
    render();
    const steps = qa("run-step");
    expect(steps.map((s) => s.dataset.state)).toEqual(["done", "done", "running"]);
    expect(steps[1].textContent).toContain("Read at block 1,204,311 (this node)");
    expect(q("run-stop")?.textContent).toBe("Stop · Esc");
    // No old spinner beside a live timeline.
    expect(host.textContent).not.toContain("running tools");
    act(() => {
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    });
    expect(stop).toHaveBeenCalledTimes(1);
    // A held key's repeats do not stop again.
    act(() => {
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", repeat: true }));
    });
    expect(stop).toHaveBeenCalledTimes(1);
    act(() => q("run-stop")!.click());
    expect(stop).toHaveBeenCalledTimes(2);
  });

  it("Running: Esc does not stop while an approval sheet is open (Esc declines there)", () => {
    const stop = vi.spyOn(store, "stopAgentTurn").mockImplementation(() => undefined);
    store.setState({ chatStatus: "tool", queue: [{ origin: "x" } as never], chatMsgs: [agent({ streaming: true, run: liveRun() })] });
    render();
    act(() => {
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    });
    expect(stop).not.toHaveBeenCalled();
  });

  it("Running: Esc is not for the chat while another dialog is open or focus is elsewhere", () => {
    const stop = vi.spyOn(store, "stopAgentTurn").mockImplementation(() => undefined);
    store.setState({ chatStatus: "tool", chatMsgs: [agent({ streaming: true, run: liveRun() })] });
    render();
    const other = document.createElement("div");
    other.setAttribute("role", "dialog");
    document.body.appendChild(other);
    act(() => {
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    });
    other.remove();
    const field = document.createElement("input");
    document.body.appendChild(field);
    field.focus();
    act(() => {
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    });
    field.remove();
    expect(stop).not.toHaveBeenCalled();
  });

  it("Keyboard: Enter on a slash runs only an exact command, not a prefix", () => {
    const newChat = vi.spyOn(store, "newChat").mockReturnValue(true);
    const send = vi.spyOn(store, "sendChat").mockResolvedValue();
    render();
    const input = q("chat-input") as HTMLTextAreaElement;
    input.value = "/ne";
    act(() => {
      input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    });
    expect(newChat).not.toHaveBeenCalled();
    input.value = "/new";
    act(() => {
      input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    });
    expect(newChat).toHaveBeenCalledTimes(1);
    expect(send).toHaveBeenCalledTimes(1);
    expect(send).toHaveBeenCalledWith("/ne");
  });

  it("Running: typing mid-run offers Steer now or Queue for after", () => {
    const steer = vi.spyOn(store, "steerChat").mockImplementation(() => undefined);
    const queue = vi.spyOn(store, "queueChat").mockImplementation(() => undefined);
    store.setState({ chatStatus: "thinking", chatMsgs: [agent({ streaming: true, run: liveRun() })] });
    render();
    const input = q("chat-input") as HTMLTextAreaElement;
    input.value = "also skip Fridays";
    act(() => {
      input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    });
    expect(q("steer-offer")?.textContent).toContain("also skip Fridays");
    act(() => q("steer-now")!.click());
    expect(steer).toHaveBeenCalledWith("also skip Fridays");
    input.value = "and Saturdays";
    act(() => q("chat-send")!.click());
    act(() => q("queue-after")!.click());
    expect(queue).toHaveBeenCalledWith("and Saturdays");
  });

  it("Running: queued follow-ups are listed and can be removed", () => {
    const drop = vi.spyOn(store, "dropQueuedChat").mockImplementation(() => undefined);
    store.setState({ chatStatus: "thinking", chatQueue: [{ id: "q1", text: "later question", steer: false, threadId: store.state.chatThreadId }], chatMsgs: [agent({ streaming: true, run: liveRun() })] });
    render();
    expect(qa("queued-msg")[0].textContent).toContain("later question");
    act(() => (host.querySelector('[aria-label="Remove queued message: later question"]') as HTMLElement).click());
    expect(drop).toHaveBeenCalledWith("q1");
  });

  it("Success: a finished run collapses, and receipts list what it read with the block", () => {
    store.setState({ chatMsgs: [you("Network status"), agent({ text: "Your node is synced.", run: doneRun(), model: "Local 8B" })] });
    render();
    expect(q("run-timeline")?.textContent).toContain("Worked for 6.0 s");
    expect(qa("run-step")).toHaveLength(4);
    act(() => q("run-toggle")!.click());
    expect(qa("run-step")).toHaveLength(0);
    expect(q("run-toggle")?.getAttribute("aria-expanded")).toBe("false");
    const receipts = qa("run-receipt").map((r) => r.textContent ?? "");
    expect(receipts[0]).toContain("Read this node's status");
    expect(receipts[0]).toContain("at block 1,204,311 (this node)");
    expect(host.textContent).toContain("Agent · Local 8B");
  });

  it("Unavailable: a chain read that does not report its block shows a dash with the reason, never 0", () => {
    let r = runToolStart(newRun(0), "c", "staking_status", 1, "core");
    r = runEnd(runToolEnd(r, "c", "staking_status", "ok", "{}", 2), "answered", 3);
    store.setState({ height: 1204311, chatMsgs: [agent({ text: "x", run: r })] });
    render();
    const step = qa("run-step")[0];
    expect(step.querySelector('[data-unknown="true"]')?.textContent).toContain("this read does not report the block it ran at");
    // The local node height is never stamped on a read that did not run there.
    expect(step.textContent).not.toContain("1,204,311");
    expect(qa("run-receipt")[0].querySelector('[data-unknown="true"]')?.textContent).toContain("does not report the block");
  });

  it("Unavailable: an unknown duration shows a dash with its reason", () => {
    let r = runToolStart(newRun(0), "c", "mcp__fs__read", 5, "events");
    r = runEnd(runToolEnd(r, "c", "mcp__fs__read", "ok", "x", 5), "answered", 9);
    store.setState({ chatMsgs: [agent({ text: "x", run: r })] });
    render();
    const d = qa("run-step-duration")[0];
    expect(d.textContent).toContain("—");
    expect(d.textContent).toContain("start and end arrived together");
  });

  it("Failure: a refused tool step says refused with core's reason", () => {
    let r = runToolStart(newRun(0), "c", "contract_deploy", 1, "events");
    r = runEnd(runToolEnd(r, "c", "contract_deploy", "denied", "declined: the member said no", 2), "answered", 3);
    store.setState({ chatMsgs: [agent({ text: "Not deployed.", run: r })] });
    render();
    const step = qa("run-step")[0];
    expect(step.dataset.state).toBe("refused");
    expect(step.textContent).toContain("refused");
    expect(step.textContent).toContain("the member said no");
  });

  it("Failure: model down, Hermes lost and stopped each get their own sentence and fix", () => {
    store.setState({
      chatMsgs: [
        agent({ id: "e1", error: "model request failed: connection refused", retryText: "hi" }),
        agent({ id: "e2", error: "the agent session stopped responding", retryText: "hi" }),
        agent({ id: "e3", error: "stopped by you", retryText: "hi" }),
      ],
    });
    render();
    const f = qa("chat-failure");
    expect(f.map((x) => x.dataset.kind)).toEqual(["model_down", "hermes_lost", "stopped"]);
    expect(f[0].textContent).toContain("The model did not answer");
    expect(f[0].querySelector('[data-testid="failure-pick_model"]')).not.toBeNull();
    expect(f[1].textContent).toContain("Hermes stopped answering");
    expect(f[2].textContent).toContain("You stopped this reply");
    for (const x of f) expect(x.textContent ?? "").not.toMatch(/!/);
  });

  it("Failure: Hermes is not running, with Start Hermes", async () => {
    const start = vi.spyOn(agentSlice, "startRuntime").mockResolvedValue(null);
    store.setState({ chatHermesDown: { why: "stopped" } });
    render();
    expect(q("hermes-down")?.textContent).toContain("Hermes is not running");
    expect(q("hermes-down")?.getAttribute("role")).toBe("alert");
    await act(async () => q("start-hermes")!.click());
    expect(start).toHaveBeenCalledWith("hermes");
  });

  it("Authority: HIC-1 by default; a named envelope shows what is left of its caps", () => {
    render();
    expect(q("authority-badge")?.dataset.level).toBe("HIC-1");
    expect(q("authority-badge")?.textContent).toContain("Every action asks you");
    expect(q("authority-badge")?.querySelector("[data-hic]")?.getAttribute("data-hic")).toBe("HIC-1");
    act(() => setEnvelopeForAuthority({ on: true, name: "Small rounds", caps: [{ label: "joins this week", left: "1 of 3" }] }));
    expect(q("authority-badge")?.dataset.level).toBe("HIC-2");
    expect(q("authority-badge")?.textContent).toContain("Small rounds · 1 of 3 joins this week");
  });

  it("Keyboard: Cmd K or Ctrl K opens the command bar; slash lists commands", () => {
    render();
    act(() => {
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "k", code: "KeyK", metaKey: true, ctrlKey: true }));
    });
    // Exactly one modifier counts (the platform's); with both held nothing opens.
    expect(q("command-bar")).toBeNull();
    act(() => q("open-command-bar")!.click());
    expect(q("command-bar")).not.toBeNull();
    expect(q("cmd-new")).not.toBeNull();
    expect(q("cmd-stop")).toBeNull(); // Stop is listed only while a run is live
    act(() => {
      q("command-bar")!.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    });
    expect(q("command-bar")).toBeNull();
    const input = q("chat-input") as HTMLTextAreaElement;
    const setter = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!;
    act(() => {
      setter.call(input, "/mo");
      input.dispatchEvent(new Event("input", { bubbles: true }));
    });
    expect(q("slash-menu")?.textContent).toContain("/model");
  });

  it("Accessibility: the transcript is a polite log, busy while streaming; steps are announced once", () => {
    store.setState({ chatStatus: "tool", chatMsgs: [agent({ streaming: true, run: liveRun() })] });
    render();
    const log = q("chat-messages")!;
    expect(log.getAttribute("role")).toBe("log");
    expect(log.getAttribute("aria-live")).toBe("polite");
    expect(log.getAttribute("aria-busy")).toBe("true");
    const status = q("run-announce")!;
    expect(status.getAttribute("aria-live")).toBe("polite");
    expect(status.textContent).toBe("Read the round coordinator.");
  });
});
