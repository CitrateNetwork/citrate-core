// FL boards 01 and 07: the round draft card is core-rendered, and its states say what happened.
import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { RoundDraftCard } from "./RoundDraftCard";
import type { ChatRoundDraft } from "../shell/state";

(globalThis as unknown as { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
const BANNED = /\b(earn|earning|earnings|income|yield|APY|APR|profit|USD|HITL)\b|!/i;

describe("RoundDraftCard", () => {
  let host: HTMLDivElement;
  let root: Root;
  beforeEach(() => {
    host = document.createElement("div");
    document.body.appendChild(host);
    root = createRoot(host);
  });
  afterEach(() => {
    act(() => root.unmount());
    host.remove();
  });
  const render = (card: ChatRoundDraft, onOpen = vi.fn(), onDecline = vi.fn()) => {
    act(() => root.render(<RoundDraftCard card={card} onOpen={onOpen} onDecline={onDecline} />));
    return { onOpen, onDecline };
  };
  const q = (id: string) => host.querySelector(`[data-testid="${id}"]`) as HTMLElement | null;

  it("Pending: the facts, what is still missing, and the member's two choices", () => {
    const { onOpen, onDecline } = render({ id: "d1", state: "pending", draft: { template: "tune", budgetSalt: "600", rateSalt: "1.50", maxWorkers: 8 } });
    const t = host.textContent ?? "";
    expect(t).toContain("Rendered by Citrate Core. Hermes cannot press these buttons.");
    expect(t).toContain("600 SALT");
    expect(t).toContain("400 leases");
    expect(t).toContain("not set; you fill it in Open a round");
    expect(q("round-draft-problems")?.textContent).toContain("Choose a group");
    expect(t).not.toMatch(BANNED);
    act(() => q("round-draft-open")!.click());
    act(() => q("round-draft-decline")!.click());
    expect(onOpen).toHaveBeenCalledTimes(1);
    expect(onDecline).toHaveBeenCalledTimes(1);
  });

  it("Opened, declined and expired offer no buttons", () => {
    for (const [state, word] of [["opened", "Opened in Open a round by you"], ["declined", "Not now · nothing was saved"], ["expired", "From an earlier session"]] as const) {
      render({ id: "d1", state, draft: {} });
      expect(host.textContent).toContain(word);
      expect(q("round-draft-open")).toBeNull();
    }
  });
});
