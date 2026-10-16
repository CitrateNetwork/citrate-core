// HUP-S10.3 (US-10.3 AC1) — the widget frame is a sandboxed iframe on the widget scheme, and the
// gallery templates only use the bridge. The frame's sandbox flags are the boundary: scripts only,
// no same-origin, no popups, no forms, no top navigation.
import { describe, it, expect, vi, afterEach } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { WidgetFrame, WIDGET_SANDBOX } from "./WidgetFrame";
import { WIDGET_TEMPLATES } from "./gallery";
import { WIDGET_QUERIES, type WidgetSources } from "./catalog";
import type { WidgetMeta } from "./api";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

vi.mock("@tauri-apps/api/core", async (orig) => ({
  ...(await orig<typeof import("@tauri-apps/api/core")>()),
  convertFileSrc: (id: string, scheme: string) => `${scheme}://localhost/${id}`,
}));

const sources: WidgetSources = {
  context: () => ({ height: 5, peers: 1, finalityAge: 2, nodeState: "synced", staked: 0, liquid: 0, claimable: 0 }),
  model: () => ({ label: "m", id: null }),
  daemons: () => ({ allPaused: false, total: 0, running: 0, paused: 0, budgetUsedUp: 0 }),
};
const meta: WidgetMeta = { id: "0123456789abcdef", name: "Block height", description: "", queries: ["node.status"], author: "gallery", createdMs: 1, bytes: 10 };

let root: Root | null = null;
let host: HTMLDivElement | null = null;
afterEach(() => {
  act(() => root?.unmount());
  host?.remove();
  root = null;
  host = null;
});

describe("WidgetFrame", () => {
  it("is a sandboxed iframe with scripts only, on the widget scheme, sending no referrer", () => {
    host = document.createElement("div");
    document.body.appendChild(host);
    root = createRoot(host);
    act(() => root?.render(<WidgetFrame widget={meta} sources={sources} />));
    const f = host.querySelector("iframe") as HTMLIFrameElement;
    expect(f.getAttribute("sandbox")).toBe("allow-scripts");
    expect(WIDGET_SANDBOX).toBe("allow-scripts");
    for (const flag of ["allow-same-origin", "allow-top-navigation", "allow-popups", "allow-forms", "allow-modals", "allow-downloads"]) {
      expect(f.getAttribute("sandbox")).not.toContain(flag);
    }
    expect(f.getAttribute("src")).toBe("citrate-widget://localhost/0123456789abcdef");
    expect(f.getAttribute("referrerpolicy")).toBe("no-referrer");
    expect(f.getAttribute("allow")).toBe("");
    expect(f.getAttribute("title")).toContain("Block height");
  });

  it("answers only its own frame's messages, through the host bridge", () => {
    host = document.createElement("div");
    document.body.appendChild(host);
    root = createRoot(host);
    const onQuery = vi.fn();
    act(() => root?.render(<WidgetFrame widget={meta} sources={sources} onQuery={onQuery} />));
    const f = host.querySelector("iframe") as HTMLIFrameElement;
    const win = f.contentWindow as Window;
    const post = vi.spyOn(win, "postMessage").mockImplementation(() => undefined);
    act(() => {
      window.dispatchEvent(new MessageEvent("message", { data: { v: 1, type: "widget.query", id: 1, query: "node.status" }, source: win }));
      // From the page itself (not the frame): ignored.
      window.dispatchEvent(new MessageEvent("message", { data: { v: 1, type: "widget.query", id: 2, query: "node.status" }, source: window }));
    });
    expect(post).toHaveBeenCalledTimes(1);
    expect(post.mock.calls[0][0]).toMatchObject({ type: "widget.result", id: 1, ok: true, data: { height: 5 } });
    expect(onQuery).toHaveBeenCalledWith(meta.id, "node.status", true);
  });
});

describe("gallery templates", () => {
  it("declare only catalog queries and use only the bridge", () => {
    expect(WIDGET_TEMPLATES.length).toBeGreaterThanOrEqual(4);
    for (const t of WIDGET_TEMPLATES) {
      for (const q of t.queries) expect(WIDGET_QUERIES).toContain(q);
      for (const banned of ["fetch(", "XMLHttpRequest", "WebSocket", "__TAURI", "invoke", "http://", "https://", "<iframe", "window.open", "top.location", "parent.location"]) {
        expect(t.html, `${t.key} uses ${banned}`).not.toContain(banned);
      }
      expect(t.html).toContain("citrate.query(");
      // Data is written as text, never parsed as HTML.
      expect(t.html).toContain("textContent");
      expect(new TextEncoder().encode(t.html).length).toBeLessThan(64 * 1024);
    }
  });
});
