// CORE-N3 follow-up: Node › Advanced "Old network data". Data source: the node data dir, through
// node_old_network_data (list with sizes) and node_remove_old_network_data (one backup, at the
// size the member confirmed; Rust refuses anything else, chain_backup.rs).
import { afterEach, describe, expect, it, vi } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { OldNetworkData, formatBytes, type OldNetworkDataReads } from "./OldNetworkData";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let root: Root | null = null;
let host: HTMLDivElement | null = null;
afterEach(() => {
  act(() => root?.unmount());
  host?.remove();
  root = null;
  host = null;
});

const BAK = { name: "chain.r1006.bak", bytes: 41_234_567_890, files: 1_204 };

async function mount(reads: OldNetworkDataReads, mode: "tauri" | "sim" = "tauri") {
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
  await act(async () => {
    root?.render(<OldNetworkData reads={reads} mode={mode} />);
  });
  return host;
}
const click = async (el: Element | null | undefined) => {
  expect(el).toBeTruthy();
  await act(async () => {
    (el as HTMLElement).click();
  });
};
const byTestId = (h: HTMLElement, id: string) => h.querySelector(`[data-testid="${id}"]`);

describe("Node › Advanced: Remove old network data", () => {
  it("formats sizes in decimal units", () => {
    expect(formatBytes(0)).toBe("0 B");
    expect(formatBytes(999)).toBe("999 B");
    expect(formatBytes(1_500_000)).toBe("1.5 MB");
    expect(formatBytes(41_234_567_890)).toBe("41.2 GB");
  });

  it("shows each backup with its size; one confirmation removes it at that size", async () => {
    const list = vi.fn(async () => [BAK]);
    const remove = vi.fn(async () => BAK.bytes);
    const h = await mount({ list, remove });
    expect(h.textContent).toContain("chain.r1006.bak");
    expect(h.textContent).toContain("41.2 GB");
    // Nothing is removed by the first click: it asks.
    await click(byTestId(h, "old-data-remove-chain.r1006.bak"));
    expect(remove).not.toHaveBeenCalled();
    expect(h.textContent).toMatch(/wallet, keys and the current chain are not touched/i);
    // Cancel leaves it.
    await click(byTestId(h, "old-data-cancel"));
    expect(remove).not.toHaveBeenCalled();
    // Ask again, confirm once.
    list.mockResolvedValueOnce([BAK]).mockResolvedValueOnce([]);
    await click(byTestId(h, "old-data-remove-chain.r1006.bak"));
    await click(byTestId(h, "old-data-confirm"));
    expect(remove).toHaveBeenCalledTimes(1);
    expect(remove).toHaveBeenCalledWith("chain.r1006.bak", BAK.bytes);
    expect(h.textContent).toMatch(/Removed chain\.r1006\.bak/);
  });

  it("shows a refusal from the app as an error and keeps the row", async () => {
    const list = vi.fn(async () => [BAK]);
    const remove = vi.fn(async () => {
      throw new Error("the backup changed since it was read");
    });
    const h = await mount({ list, remove });
    await click(byTestId(h, "old-data-remove-chain.r1006.bak"));
    await click(byTestId(h, "old-data-confirm"));
    expect(h.textContent).toContain("the backup changed since it was read");
    expect(h.textContent).toContain("chain.r1006.bak");
  });

  it("says when there is none, and renders nothing actionable outside the desktop app", async () => {
    const none = await mount({ list: async () => [], remove: vi.fn() });
    expect(none.textContent).toMatch(/No old network data/);
    act(() => root?.unmount());
    host?.remove();
    const list = vi.fn(async () => [BAK]);
    const sim = await mount({ list, remove: vi.fn() }, "sim");
    expect(list).not.toHaveBeenCalled();
    expect(byTestId(sim, "old-data-remove-chain.r1006.bak")).toBeNull();
  });
});
