// HUP-S2.3: the "Signed for you" notice (ADR D4 after-the-fact visibility #1): one non-modal
// notice per automatic sign-in, with a one-click "Revoke this budget".
import { describe, it, expect, afterEach, vi } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { SignedForYou, MAX_NOTICES } from "./SignedForYou";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let root: Root | null = null;
let host: HTMLDivElement | null = null;
afterEach(() => {
  act(() => root?.unmount());
  host?.remove();
  root = null;
  host = null;
});
const tick = () => act(async () => { await new Promise((r) => setTimeout(r, 0)); });

function fakeEvents() {
  let handler: ((p: unknown) => void) | null = null;
  const unlisten = vi.fn();
  return {
    subscribe: vi.fn(async (h: (p: unknown) => void) => { handler = h; return unlisten; }),
    emit: (p: unknown) => act(() => handler?.(p)),
    unlisten,
  };
}

async function render(el: React.ReactElement) {
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
  await act(async () => { root?.render(el); });
  await tick();
  return host;
}

const N = { origin: "https://app.example.org", budgetId: 4, recordId: 12, remaining: 2 };

describe("HUP-S2.3 Signed for you", () => {
  it("shows nothing until core announces an automatic sign-in", async () => {
    const ev = fakeEvents();
    const el = await render(<SignedForYou subscribe={ev.subscribe} revoke={vi.fn()} />);
    expect(el.textContent).toBe("");
    await ev.emit({ bogus: true });
    expect(el.textContent).toBe("");
    await ev.emit(N);
    expect(el.textContent).toContain("Hermes signed you in to https://app.example.org");
    expect(el.textContent).toContain("2 automatic sign-ins left");
    expect(el.querySelector('[role="status"]')).not.toBeNull();
  });

  it("revokes that budget in one click and says what happens next", async () => {
    const ev = fakeEvents();
    const revoke = vi.fn(async () => undefined);
    const el = await render(<SignedForYou subscribe={ev.subscribe} revoke={revoke} />);
    await ev.emit(N);
    const btn = Array.from(el.querySelectorAll("button")).find((b) => b.textContent === "Revoke this budget");
    await act(async () => { btn?.click(); });
    await tick();
    expect(revoke).toHaveBeenCalledWith(4);
    expect(el.textContent).toContain("Budget revoked");
    expect(Array.from(el.querySelectorAll("button")).some((b) => b.textContent === "Revoke this budget")).toBe(false);
  });

  it("shows a failed revoke instead of hiding it", async () => {
    const ev = fakeEvents();
    const el = await render(<SignedForYou subscribe={ev.subscribe} revoke={vi.fn(async () => { throw new Error("the budget file could not be saved"); })} />);
    await ev.emit(N);
    const btn = Array.from(el.querySelectorAll("button")).find((b) => b.textContent === "Revoke this budget");
    await act(async () => { btn?.click(); });
    await tick();
    expect(el.textContent).toContain("the budget file could not be saved");
  });

  it("keeps at most a few notices, can be dismissed, and stops listening on unmount", async () => {
    const ev = fakeEvents();
    const el = await render(<SignedForYou subscribe={ev.subscribe} revoke={vi.fn()} />);
    for (let i = 0; i < MAX_NOTICES + 2; i++) await ev.emit({ ...N, recordId: i });
    expect(el.querySelectorAll('[data-register="charter"]').length).toBe(MAX_NOTICES);
    const dismiss = Array.from(el.querySelectorAll("button")).find((b) => b.textContent === "Dismiss");
    await act(async () => { dismiss?.click(); });
    expect(el.querySelectorAll('[data-register="charter"]').length).toBe(MAX_NOTICES - 1);
    act(() => root?.unmount());
    root = null;
    expect(ev.unlisten).toHaveBeenCalled();
  });
});
