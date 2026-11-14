// 0.5.4 (owner request 2026-11-14) — the sidebar and the Hermes chat's right column (widgets,
// daemons, transactions, tutorials) both collapse: a toggle with aria-expanded, a keyboard shortcut
// (⌘B / Ctrl+B for the sidebar, ⌥⌘B / Ctrl+Alt+B for the column), and the choice is persisted.
import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";

vi.mock("wagmi", () => ({
  useBlockNumber: () => ({ data: undefined, isPending: true, error: null }),
  useReadContract: () => ({ data: undefined }),
}));
vi.mock("../components/AgentChat", () => ({ AgentChat: () => null }));
vi.mock("../widgets/WidgetsPanel", () => ({ WidgetsPanel: () => <div data-testid="widgets-panel" /> }));
vi.mock("../daemons/DaemonsPanel", () => ({ DaemonsPanel: () => null }));

import { Sidebar } from "./Sidebar";
import { Dashboard } from "../surfaces/Dashboard";
import { layoutShortcut, shortcutLabel, useLayoutShortcut } from "./layoutShortcuts";
import { store } from "./store";
import { freshState, loadState, PERSIST_KEYS, PERSIST_SCOPE, STORAGE_KEY } from "./state";

(globalThis as unknown as { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const key = (over: Partial<KeyboardEvent>) => ({ code: "KeyB", key: "b", metaKey: false, ctrlKey: false, altKey: false, shiftKey: false, ...over });

describe("layoutShortcut (pure)", () => {
  it("⌘B toggles the sidebar and ⌥⌘B the right column on macOS", () => {
    expect(layoutShortcut(key({ metaKey: true }), true)).toBe("sidebar");
    // ⌥B types "∫" on a Mac: the physical key still matches.
    expect(layoutShortcut(key({ metaKey: true, altKey: true, key: "∫" }), true)).toBe("rail");
  });
  it("Ctrl+B and Ctrl+Alt+B elsewhere", () => {
    expect(layoutShortcut(key({ ctrlKey: true }), false)).toBe("sidebar");
    expect(layoutShortcut(key({ ctrlKey: true, altKey: true }), false)).toBe("rail");
  });
  it("leaves other keys alone (Ctrl+B on a Mac is a text cursor move; Shift and plain B never match)", () => {
    expect(layoutShortcut(key({ ctrlKey: true }), true)).toBeNull();
    expect(layoutShortcut(key({ metaKey: true, shiftKey: true }), true)).toBeNull();
    expect(layoutShortcut(key({}), true)).toBeNull();
    expect(layoutShortcut(key({ metaKey: true, code: "KeyN", key: "n" }), true)).toBeNull();
  });
  it("the tooltip names the platform's shortcut", () => {
    expect(shortcutLabel("sidebar", true)).toBe("⌘B");
    expect(shortcutLabel("rail", false)).toBe("Ctrl+Alt+B");
  });
});

describe("collapse state is persisted with the member's other settings", () => {
  beforeEach(() => localStorage.clear());
  it("both flags are device-scoped persisted keys, open by default", () => {
    expect(PERSIST_KEYS).toContain("sidebarCollapsed");
    expect(PERSIST_KEYS).toContain("hermesRailCollapsed");
    expect(PERSIST_SCOPE.sidebarCollapsed).toBe("device");
    expect(freshState("p1").sidebarCollapsed).toBe(false);
    expect(freshState("p1").hermesRailCollapsed).toBe(false);
  });
  it("a toggle is written to storage and restored on the next launch", () => {
    vi.useFakeTimers();
    try {
      store.setState({ ...freshState("p1") });
      store.toggleSidebar();
      store.toggleHermesRail();
      vi.advanceTimersByTime(1000);
      const saved = JSON.parse(localStorage.getItem(STORAGE_KEY) || "{}");
      expect(saved.sidebarCollapsed).toBe(true);
      expect(saved.hermesRailCollapsed).toBe(true);
      const next = loadState(store.state.netKey);
      expect(next.sidebarCollapsed).toBe(true);
      expect(next.hermesRailCollapsed).toBe(true);
    } finally {
      vi.useRealTimers();
    }
  });
});

describe("collapse toggles in the UI", () => {
  let host: HTMLDivElement;
  let root: Root;
  beforeEach(() => {
    host = document.createElement("div");
    document.body.appendChild(host);
    root = createRoot(host);
    store.setState({ ...freshState("p1"), stage: "done" });
    vi.spyOn(store, "refreshProducers").mockResolvedValue(undefined as never);
    vi.spyOn(store, "save").mockImplementation(() => {});
  });
  afterEach(() => {
    act(() => root.unmount());
    host.remove();
    vi.restoreAllMocks();
  });
  const render = (el: () => JSX.Element) => act(() => root.render(el()));
  const q = (id: string) => host.querySelector(`[data-testid="${id}"]`) as HTMLElement | null;

  it("the sidebar toggle collapses to icons and back, with aria-expanded", () => {
    const view = () => <Sidebar store={store} s={store.state} />;
    render(view);
    const toggle = q("sidebar-toggle")!;
    expect(toggle.getAttribute("aria-expanded")).toBe("true");
    expect(toggle.getAttribute("aria-controls")).toBe("app-sidebar-nav");
    expect(host.textContent).toContain("Wallet");
    act(() => toggle.click());
    expect(store.state.sidebarCollapsed).toBe(true);
    render(view);
    expect(q("sidebar-toggle")!.getAttribute("aria-expanded")).toBe("false");
    expect(q("app-sidebar")!.getAttribute("data-collapsed")).toBe("true");
    // Labels hide, but every place keeps its accessible name.
    expect(host.textContent).not.toContain("Wallet");
    expect(host.querySelector('a[aria-label="Wallet"]')).toBeTruthy();
    act(() => q("sidebar-toggle")!.click());
    expect(store.state.sidebarCollapsed).toBe(false);
  });

  it("the right column toggle hides widgets and tutorials and brings them back, with aria-expanded", () => {
    const view = () => <Dashboard store={store} s={store.state} />;
    render(view);
    expect(q("hermes-rail")).toBeTruthy();
    expect(q("widgets-panel")).toBeTruthy();
    expect(host.textContent).toContain("Tutorials");
    const toggle = q("rail-toggle")!;
    expect(toggle.getAttribute("aria-expanded")).toBe("true");
    expect(toggle.getAttribute("aria-controls")).toBe("hermes-rail");
    act(() => toggle.click());
    expect(store.state.hermesRailCollapsed).toBe(true);
    render(view);
    expect(q("hermes-rail")).toBeNull();
    expect(q("widgets-panel")).toBeNull();
    expect(host.textContent).not.toContain("Tutorials");
    expect(q("hermes-rail-strip")).toBeTruthy();
    expect(q("rail-toggle")!.getAttribute("aria-expanded")).toBe("false");
    act(() => q("rail-toggle")!.click());
    expect(store.state.hermesRailCollapsed).toBe(false);
  });

  it("the keyboard shortcut toggles the column the hook names", () => {
    const onToggle = vi.fn();
    function Probe() {
      useLayoutShortcut("rail", onToggle);
      return null;
    }
    render(() => <Probe />);
    const ctrl = !/Mac|iPhone|iPad/i.test(navigator.platform || navigator.userAgent || "");
    const mod = ctrl ? { ctrlKey: true } : { metaKey: true };
    act(() => void window.dispatchEvent(new KeyboardEvent("keydown", { code: "KeyB", key: "b", altKey: true, ...mod })));
    expect(onToggle).toHaveBeenCalledTimes(1);
    // The sidebar's shortcut is not the column's.
    act(() => void window.dispatchEvent(new KeyboardEvent("keydown", { code: "KeyB", key: "b", ...mod })));
    expect(onToggle).toHaveBeenCalledTimes(1);
  });
});
