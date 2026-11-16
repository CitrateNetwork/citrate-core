// FL design board 01 (Hermes › Chat), part 2: edit, retry and branch; "What Hermes can see";
// the core-rendered worker-terms proposal card (Pending, Success, Failure, declined).
import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { AgentChat } from "./AgentChat";
import { store } from "../shell/store";
import { freshState, type ChatMsg, type ChatProposal } from "../shell/state";
import { annotatedAgentTools } from "../agent/toolAnnotations";
import { setEnvelopeForAuthority } from "../fl/authority";

(globalThis as unknown as { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const BANNED = /\b(earn|earning|earnings|income|yield|APY|APR|profit|HITL)\b|!/i;
const msg = (id: string, who: string, text: string, extra: Partial<ChatMsg> = {}): ChatMsg => ({ id, who, text, chips: [], streaming: false, ...extra });
const proposal = (state: ChatProposal["state"], extra: Partial<ChatProposal> = {}): ChatProposal => ({
  id: "p1",
  kind: "worker_terms",
  terms: { floorSaltPerUnit: "1.20", hours: { start: "23:00", end: "07:00" }, weekdaysOnly: true, mainsOnly: true, dataClasses: [] },
  state,
  ...extra,
});

describe("board 01 Hermes chat, part 2", () => {
  let host: HTMLDivElement;
  let root: Root;
  beforeEach(() => {
    host = document.createElement("div");
    document.body.appendChild(host);
    root = createRoot(host);
    store.setState({
      ...freshState("p1"),
      chatStatus: "ready",
      chatQueue: [],
      chatHermesDown: null,
      chatMsgs: [msg("u1", "You", "Network status"), msg("a1", "Agent", "Synced.")],
    });
  });
  afterEach(() => {
    act(() => root.unmount());
    host.remove();
    vi.restoreAllMocks();
  });
  const render = () => act(() => root.render(<AgentChat store={store} s={store.state} />));
  const q = (id: string) => host.querySelector(`[data-testid="${id}"]`) as HTMLElement | null;
  const qa = (id: string) => Array.from(host.querySelectorAll(`[data-testid="${id}"]`)) as HTMLElement[];

  it("Edit puts the message in the composer as a branch; Send branches with the new text", () => {
    const branch = vi.spyOn(store, "branchChat").mockResolvedValue(true);
    render();
    act(() => q("msg-edit")!.click());
    const input = q("chat-input") as HTMLTextAreaElement;
    expect(input.value).toBe("Network status");
    expect(q("editing-banner")?.textContent).toContain("The original stays in History");
    input.value = "Network status and peers";
    act(() => q("chat-send")!.click());
    expect(branch).toHaveBeenCalledWith("u1", { text: "Network status and peers" });
    expect(q("editing-banner")).toBeNull();
  });

  it("Up in an empty composer edits your last message; Esc cancels", () => {
    render();
    const input = q("chat-input") as HTMLTextAreaElement;
    act(() => {
      input.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowUp", bubbles: true }));
    });
    expect(input.value).toBe("Network status");
    expect(q("editing-banner")).not.toBeNull();
    act(() => {
      input.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    });
    expect(q("editing-banner")).toBeNull();
    expect(input.value).toBe("");
  });

  it("an answer offers Retry, Retry with another model and Branch from here; none while a run is live", () => {
    const retry = vi.spyOn(store, "retryAsBranch").mockResolvedValue(true);
    const branch = vi.spyOn(store, "branchChat").mockResolvedValue(true);
    render();
    act(() => q("msg-retry")!.click());
    expect(retry).toHaveBeenCalledWith("a1");
    act(() => q("msg-branch")!.click());
    expect(branch).toHaveBeenCalledWith("a1", { includeFrom: true });
    act(() => q("msg-retry-with")!.click());
    expect(q("retry-models")).not.toBeNull();
    act(() => store.setState({ chatStatus: "thinking" }));
    render();
    expect((q("msg-retry") as HTMLButtonElement).disabled).toBe(true);
    expect(q("msg-edit")).toBeNull();
  });

  it("What Hermes can see: from the session's tools; keys never", () => {
    vi.spyOn(store, "chatToolNames").mockReturnValue(annotatedAgentTools().map((t) => t.function.name));
    vi.spyOn(store, "activeProviderKind").mockReturnValue("sidecar");
    render();
    act(() => q("open-context")!.click());
    const rows = qa("context-row");
    const by = (title: string) => rows.find((r) => r.textContent?.includes(title))!;
    expect(by("This node").dataset.can).toBe("true");
    expect(by("Your groups").textContent).toContain("not their messages");
    expect(by("Your wallet keys").dataset.can).toBe("false");
    expect(by("Datasets").dataset.can).toBe("false");
  });

  it("What Hermes can see when Hermes is not running: nothing", () => {
    vi.spyOn(store, "chatToolNames").mockReturnValue([]);
    store.setState({ chatHermesDown: { why: "stopped" } });
    render();
    act(() => q("open-context")!.click());
    expect(qa("context-row").every((r) => r.dataset.can === "false")).toBe(true);
    expect(q("hermes-context")?.textContent).toContain("no tools");
  });

  it("Pending: the proposal card is rendered by core, with its facts and the member's three choices", () => {
    const confirm = vi.spyOn(store, "confirmProposal").mockResolvedValue();
    const decline = vi.spyOn(store, "declineProposal").mockImplementation(() => undefined);
    store.setState({ chatMsgs: [msg("a1", "Agent", "I drafted a policy.", { proposal: proposal("pending") })] });
    render();
    const card = q("policy-proposal")!;
    expect(card.textContent).toContain("Rendered by Citrate Core. Hermes cannot press these buttons.");
    expect(card.textContent).toContain("1.20 SALT per unit");
    expect(card.textContent).toContain("23:00 to 07:00, Monday to Friday");
    expect(card.querySelector("[data-hic]")?.getAttribute("data-hic")).toBe("HIC-1");
    expect(card.textContent ?? "").not.toMatch(BANNED);
    act(() => q("proposal-confirm")!.click());
    expect(confirm).toHaveBeenCalledWith("p1");
    act(() => q("proposal-decline")!.click());
    expect(decline).toHaveBeenCalledWith("p1");
  });

  it("Success, declined and failed states say what happened", () => {
    store.setState({ chatMsgs: [msg("a1", "Agent", "x", { proposal: proposal("confirmed", { at: new Date(2026, 10, 15, 21, 14).getTime() }) })] });
    render();
    expect(q("policy-proposal")?.textContent).toContain("Confirmed by you at 21:14");
    act(() => store.setState({ chatMsgs: [msg("a1", "Agent", "x", { proposal: proposal("declined") })] }));
    render();
    expect(q("policy-proposal")?.textContent).toContain("Not now · nothing changed");
    act(() => store.setState({ chatMsgs: [msg("a1", "Agent", "x", { proposal: proposal("failed", { error: "Saving terms is not available in this build" }) })] }));
    render();
    expect(q("proposal-failed")?.textContent).toBe("Save failed, nothing changed");
    expect(q("policy-proposal")?.textContent).toContain("Saving terms is not available in this build");
    expect(q("proposal-confirm")).toBeNull();
  });

  it("Stored says only what the save receipt says; Auto-join follows the authority", () => {
    store.setState({ chatMsgs: [msg("a1", "Agent", "x", { proposal: proposal("pending") })] });
    render();
    const card = () => q("policy-proposal")!.textContent ?? "";
    expect(card()).toContain("Not saved yet");
    expect(card()).not.toContain("Sealed on this device");
    expect(card()).toContain("Off. Each join asks you (HIC-1).");
    act(() => setEnvelopeForAuthority({ on: true, name: "Small rounds", caps: [] }));
    expect(card()).toContain('Inside your envelope "Small rounds" (HIC-2)');
    act(() => setEnvelopeForAuthority(null));
    act(() => store.setState({ chatMsgs: [msg("a1", "Agent", "x", { proposal: proposal("confirmed", { at: 1, receipt: { savedAt: 1, termsHash: "0x" + "ab".repeat(32), sealed: true } }) })] }));
    render();
    expect(card()).toContain("Sealed on this device");
    act(() => store.setState({ chatMsgs: [msg("a1", "Agent", "x", { proposal: proposal("confirmed", { at: 1, receipt: { savedAt: 1, termsHash: "0x" + "ab".repeat(32), sealed: false } }) })] }));
    render();
    expect(card()).toContain("Saved on this device, not sealed");
  });

  it("an expired card offers no buttons and says to ask again", () => {
    store.setState({ chatMsgs: [msg("a1", "Agent", "x", { proposal: proposal("expired") })] });
    render();
    expect(q("proposal-expired")?.textContent).toContain("can no longer be confirmed");
    expect(q("proposal-confirm")).toBeNull();
    expect(q("proposal-reopen")).toBeNull();
  });
});
