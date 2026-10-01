// HUP-S2.9 — the file-change card under an agent reply: what changed, an Undo button while it can
// be undone, and the honest result (undone, refused with the reason, or failed).
import { describe, it, expect, vi, afterEach } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { FileChangeCard } from "./FileChangeCard";
import type { UndoCard } from "../shell/slices/agentUndo";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let root: Root | null = null;
let host: HTMLDivElement | null = null;
afterEach(() => {
  act(() => root?.unmount());
  host?.remove();
  root = null;
  host = null;
});
function render(el: React.ReactElement) {
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
  act(() => root?.render(el));
  return host;
}
const q = (el: HTMLElement, id: string) => el.querySelector(`[data-testid="${id}"]`) as HTMLElement | null;

const card = (over: Partial<UndoCard> = {}): UndoCard => ({
  session: "s4-cafe", seq: 3, tool: "fs_edit", paths: ["/w/proj/src/lib.rs"], msgId: "m2", state: "applied", note: null, ...over,
});

describe("HUP-S2.9 file-change card", () => {
  it("names the change and offers Undo", () => {
    const onUndo = vi.fn();
    const el = render(<FileChangeCard card={card()} onUndo={onUndo} />);
    expect(q(el, "file-change-card")?.textContent).toContain("Edited");
    expect(q(el, "file-change-card")?.textContent).toContain("/w/proj/src/lib.rs");
    const b = q(el, "file-change-undo") as HTMLButtonElement;
    expect(b.disabled).toBe(false);
    act(() => b.click());
    expect(onUndo).toHaveBeenCalledWith("s4-cafe", 3);
  });

  it("a rename shows both paths", () => {
    const el = render(<FileChangeCard card={card({ tool: "fs_rename", paths: ["/w/a.txt", "/w/b.txt"] })} onUndo={vi.fn()} />);
    expect(q(el, "file-change-card")?.textContent).toMatch(/Renamed.*\/w\/a\.txt.*\/w\/b\.txt/);
  });

  it("while undoing the button cannot be pressed again", () => {
    const el = render(<FileChangeCard card={card({ state: "undoing" })} onUndo={vi.fn()} />);
    const b = q(el, "file-change-undo") as HTMLButtonElement;
    expect(b.disabled).toBe(true);
    expect(b.textContent).toMatch(/Undoing/);
  });

  it("once undone there is no Undo button, only the result", () => {
    const el = render(<FileChangeCard card={card({ state: "undone", note: "Undone: restored lib.rs." })} onUndo={vi.fn()} />);
    expect(q(el, "file-change-undo")).toBeNull();
    expect(q(el, "file-change-note")?.textContent).toBe("Undone: restored lib.rs.");
  });

  it("a refusal shows the reason as an alert and allows another try", () => {
    const el = render(<FileChangeCard card={card({ state: "refused", note: "Not undone: src/lib.rs changed after the agent's edit." })} onUndo={vi.fn()} />);
    const note = q(el, "file-change-note");
    expect(note?.getAttribute("role")).toBe("alert");
    expect(note?.textContent).toContain("changed after the agent's edit");
    expect((q(el, "file-change-undo") as HTMLButtonElement).disabled).toBe(false);
  });
});
