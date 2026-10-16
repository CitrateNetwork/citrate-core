// HUP-S10.3 (US-10.3 AC1) — the main window's side of the widget bridge. A widget can only ask
// for data it declared, from the read-only catalog, through postMessage from its own frame. These
// tests are the sandbox-escape attempts the bridge must refuse.
import { describe, it, expect, vi } from "vitest";
import { createWidgetHost, type FrameLike, type WidgetHostDeps } from "./host";
import { resolveQuery, WIDGET_QUERIES, type WidgetSources } from "./catalog";

const sources: WidgetSources = {
  context: () => ({ height: 120_345, peers: 7, finalityAge: 3, nodeState: "synced", staked: 32_000, liquid: 12.5, claimable: 1.25, earningsToday: 0.5, walletAddr: "0xabc0000000000000000000000000000000000001", tier: "T1" }),
  model: () => ({ label: "Gemma 4 E4B", id: "local:gemma" }),
  daemons: () => ({ allPaused: false, total: 2, running: 1, paused: 0, budgetUsedUp: 1 }),
};

function frame(): FrameLike & { sent: unknown[] } {
  const sent: unknown[] = [];
  return { sent, postMessage: (m: unknown) => sent.push(m) };
}

function host(declared: string[], over: Partial<WidgetHostDeps> = {}) {
  const f = frame();
  const h = createWidgetHost({ widgetId: "0123456789abcdef", declared, frame: f, sources, now: () => 1_000, ...over });
  return { f, h };
}

const q = (id: number, query: string) => ({ v: 1, type: "widget.query", id, query });

describe("widget query catalog", () => {
  it("is the same list Rust serves and resolves only from app state", () => {
    expect([...WIDGET_QUERIES]).toEqual(["node.status", "wallet.summary", "model.active", "daemons.summary"]);
    expect(resolveQuery("node.status", sources)).toEqual({ height: 120_345, peers: 7, state: "synced", finalityAgeSec: 3 });
    expect(resolveQuery("model.active", sources)).toEqual({ label: "Gemma 4 E4B", id: "local:gemma" });
    expect(resolveQuery("daemons.summary", sources)).toEqual({ allPaused: false, total: 2, running: 1, paused: 0, budgetUsedUp: 1 });
  });

  it("never hands a widget the wallet address", () => {
    const w = resolveQuery("wallet.summary", sources) as Record<string, unknown>;
    expect(w).toEqual({ liquidSalt: 12.5, stakedSalt: 32_000, claimableSalt: 1.25 });
    expect(JSON.stringify(w)).not.toContain("0xabc");
  });
});

describe("widget host bridge", () => {
  it("answers a declared catalog query from its own frame", () => {
    const { f, h } = host(["node.status"]);
    h.onMessage({ source: f, data: q(1, "node.status") });
    expect(f.sent).toEqual([{ v: 1, type: "widget.result", id: 1, ok: true, data: { height: 120_345, peers: 7, state: "synced", finalityAgeSec: 3 } }]);
  });

  it("refuses a catalog query the widget did not declare", () => {
    const { f, h } = host(["node.status"]);
    h.onMessage({ source: f, data: q(2, "wallet.summary") });
    expect(f.sent).toEqual([{ v: 1, type: "widget.result", id: 2, ok: false, error: "wallet.summary was not declared by this widget" }]);
  });

  it("refuses names outside the catalog even if declared (invoke, Tauri internals, prototype keys)", () => {
    const evil = ["invoke", "__TAURI_INTERNALS__", "sign_approve", "constructor", "__proto__", "toString", "wallet.send"];
    const { f, h } = host(evil);
    evil.forEach((name, i) => h.onMessage({ source: f, data: q(i + 1, name) }));
    expect(f.sent).toHaveLength(evil.length);
    for (const m of f.sent as { ok: boolean; error: string }[]) {
      expect(m.ok).toBe(false);
      expect(m.error).toMatch(/not a widget query/);
    }
  });

  it("ignores messages from any window but its own frame", () => {
    const { f, h } = host(["node.status"]);
    const other = frame();
    h.onMessage({ source: other, data: q(1, "node.status") });
    h.onMessage({ source: null, data: q(1, "node.status") });
    expect(f.sent).toEqual([]);
    expect(other.sent).toEqual([]);
  });

  it("drops malformed messages whole", () => {
    const { f, h } = host(["node.status"]);
    for (const data of [null, "node.status", 7, {}, { v: 2, type: "widget.query", id: 1, query: "node.status" }, { v: 1, type: "invoke", id: 1, cmd: "sign_approve" }, { v: 1, type: "widget.query", id: "1", query: "node.status" }, { v: 1, type: "widget.query", id: 1.5, query: "node.status" }, { v: 1, type: "widget.query", id: 1, query: { toString: () => "node.status" } }]) {
      h.onMessage({ source: f, data });
    }
    expect(f.sent).toEqual([]);
  });

  it("limits how often a widget may ask", () => {
    let t = 0;
    const { f, h } = host(["node.status"], { now: () => t });
    for (let i = 1; i <= 40; i++) h.onMessage({ source: f, data: q(i, "node.status") });
    const sent = f.sent as { ok: boolean; error?: string }[];
    expect(sent.filter((m) => m.ok)).toHaveLength(30);
    expect(sent.filter((m) => !m.ok).every((m) => /too many/.test(m.error ?? ""))).toBe(true);
    t = 61_000;
    h.onMessage({ source: f, data: q(41, "node.status") });
    expect((f.sent.at(-1) as { ok: boolean }).ok).toBe(true);
  });

  it("stops answering once closed", () => {
    const { f, h } = host(["node.status"]);
    h.close();
    h.onMessage({ source: f, data: q(1, "node.status") });
    expect(f.sent).toEqual([]);
  });

  it("reports a source failure as an error, not invented data", () => {
    const failing: WidgetSources = { ...sources, model: () => { throw new Error("router down"); } };
    const { f, h } = host(["model.active"], { sources: failing });
    h.onMessage({ source: f, data: q(1, "model.active") });
    expect(f.sent).toEqual([{ v: 1, type: "widget.result", id: 1, ok: false, error: "model.active is unavailable right now" }]);
  });

  it("counts answered and refused queries for the Activity monitor", () => {
    const onQuery = vi.fn();
    const { f, h } = host(["node.status"], { onQuery });
    h.onMessage({ source: f, data: q(1, "node.status") });
    h.onMessage({ source: f, data: q(2, "wallet.summary") });
    expect(onQuery.mock.calls).toEqual([["0123456789abcdef", "node.status", true], ["0123456789abcdef", "wallet.summary", false]]);
  });
});
