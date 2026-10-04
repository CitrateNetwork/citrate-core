// HUP-S4.1 (US-4.1 AC2): an MCP request the sidecar holds (an effectful MCP call after taint, or a
// page a server asks to open) reaches the member as an approval card, and the decision goes back
// bound to exactly what was shown. MCP calls that never need the member are not held up.
import { describe, it, expect, vi } from "vitest";
import { createSidecarProvider, type SidecarSessionApi } from "./sidecarProvider";
import type { McpPendingView } from "../bridge/domains";
import { mcpRequestCard } from "./approvalCards";

type Ev = { seq: number; event: Record<string, unknown> };

const ARGS = '{"to":"0x52908400098527886E0F7030069857D2E4169EE7","value_wei":"1"}';

const CALL_CARD: McpPendingView = {
  id: "mcp-1",
  kind: "tool_call",
  callId: "c1",
  server: "node",
  remoteTool: "tx_propose",
  tool: "mcp__node__tx_propose",
  hic: "required",
  subject: ARGS,
  arguments: ARGS,
  hints: { readOnly: false, destructive: true, idempotent: false, openWorld: true },
  reason: "this session read untrusted content (from mcp__node__chain_head), so this action needs your explicit approval",
  warnings: [],
  expiresInSecs: 300,
};

const URL = "https://auth.example.com/connect?state=abc";
const URL_CARD: McpPendingView = {
  id: "mcp-2",
  kind: "open_url",
  callId: "c1",
  server: "web",
  remoteTool: "connect_account",
  tool: "mcp__web__connect_account",
  hic: "required",
  subject: URL,
  reason: "Connect your GitHub account.",
  url: URL,
  urlHost: "auth.example.com",
  warnings: [],
  expiresInSecs: 300,
};

/** Events served like the real sidecar: a read never consumes anything, it returns every released
 *  event after `after`. With `gated`, pages after the first are released only once every card was
 *  decided (the real sidecar holds the call until the member decides). */
function api(pages: Ev[][], cards: McpPendingView[], gated = true) {
  const a: SidecarSessionApi & { decided: unknown[] } = {
    decided: [],
    open: vi.fn(async () => "s9-beef"),
    send: vi.fn(async () => {}),
    events: vi.fn(async (_id: string, after: number) => {
      const released = gated && a.decided.length < cards.length ? pages.slice(0, 1) : pages;
      const all = released.flat();
      const fresh = all.filter((e) => e.seq > after);
      const lastSeq = all.length ? Math.max(after, all[all.length - 1].seq) : after;
      return { events: fresh, lastSeq, busy: true };
    }),
    toolResult: vi.fn(async () => {}),
    stop: vi.fn(async () => {}),
    mcpPending: vi.fn(async () => cards.filter((c) => !a.decided.some((d) => (d as { approvalId: string }).approvalId === c.id))),
    mcpDecide: vi.fn(async (_id: string, approvalId: string, allow: boolean, subject: string) => {
      a.decided.push({ approvalId, allow, subject });
    }),
  };
  return a;
}

const call = (seq: number, id: string, name: string, hic = false): Ev => ({
  seq,
  event: { type: "tool_call", step: 1, call: { id, name, arguments: "{}" }, host: "sidecar", ...(hic ? { hic: "required" } : {}) },
});
const result = (seq: number, id: string, status: string, content: string): Ev => ({
  seq,
  event: { type: "tool_result", step: 1, call_id: id, status, content },
});
const done = (seq: number): Ev => ({ seq, event: { type: "done", outcome: "answered" } });

const OPTS = { mcpPollMs: 1, mcpWaitMs: 300 };
const cb = (onMcpApproval?: (p: McpPendingView) => Promise<boolean>) => ({
  onStatus: vi.fn(),
  onToken: vi.fn(),
  onToolCall: vi.fn(async () => "ok"),
  ...(onMcpApproval ? { onMcpApproval } : {}),
});

describe("HUP-S4.1 MCP approval cards in the sidecar provider", () => {
  it("asks the member about a held MCP call and decides with exactly the arguments shown", async () => {
    const a = api([[call(1, "c1", "mcp__node__tx_propose", true)], [result(2, "c1", "ok", "pending request req-1"), done(3)]], [CALL_CARD]);
    const seen: McpPendingView[] = [];
    const p = createSidecarProvider(a, () => "p", () => [], () => null, OPTS);
    await p.send({
      messages: [{ role: "user", content: "send 1 wei" }],
      callbacks: cb(async (pending) => {
        seen.push(pending);
        return true;
      }),
    });
    expect(seen).toEqual([CALL_CARD]);
    expect(a.decided).toEqual([{ approvalId: "mcp-1", allow: true, subject: ARGS }]);
    expect(a.toolResult).not.toHaveBeenCalled();
  });

  it("a page request is decided with the URL shown; a no is a decline", async () => {
    const a = api([[call(1, "c1", "mcp__web__connect_account")], [result(2, "c1", "ok", "not connected: decline"), done(3)]], [URL_CARD]);
    const p = createSidecarProvider(a, () => "p", () => [], () => null, OPTS);
    await p.send({ messages: [{ role: "user", content: "connect" }], callbacks: cb(async () => false) });
    expect(a.decided).toEqual([{ approvalId: "mcp-2", allow: false, subject: URL }]);
  });

  it("without an approval callback every held request is declined, never allowed silently", async () => {
    const a = api([[call(1, "c1", "mcp__node__tx_propose", true)], [result(2, "c1", "denied", "declined"), done(3)]], [CALL_CARD]);
    const p = createSidecarProvider(a, () => "p", () => [], () => null, OPTS);
    await p.send({ messages: [{ role: "user", content: "x" }], callbacks: cb() });
    expect(a.decided).toEqual([{ approvalId: "mcp-1", allow: false, subject: ARGS }]);
  });

  it("an MCP call that needs no one is not held up: the provider stops looking once it is answered", async () => {
    const a = api([[call(1, "c1", "mcp__node__chain_head"), result(2, "c1", "ok", "height 78024"), done(3)]], [], false);
    const ask = vi.fn(async () => true);
    const p = createSidecarProvider(a, () => "p", () => [], () => null, OPTS);
    await p.send({ messages: [{ role: "user", content: "head?" }], callbacks: cb(ask) });
    expect(ask).not.toHaveBeenCalled();
    expect(a.mcpPending).not.toHaveBeenCalled();
    expect(a.decided).toEqual([]);
  });

  it("a decision that does not reach Hermes is reported, not retried as an approval", async () => {
    const a = api([[call(1, "c1", "mcp__node__tx_propose", true)], [result(2, "c1", "denied", "declined"), done(3)]], [CALL_CARD]);
    // The card expired in the sidecar meanwhile: the decision is refused and the call declined.
    a.mcpDecide = vi.fn(async (_id: string, approvalId: string, allow: boolean, subject: string) => {
      a.decided.push({ approvalId, allow, subject, refused: true });
      throw new Error("MCP_DECISION_REFUSED: no longer waiting");
    });
    const notices: string[] = [];
    const p = createSidecarProvider(a, () => "p", () => [], () => null, OPTS);
    await p.send({
      messages: [{ role: "user", content: "x" }],
      callbacks: {
        ...cb(async () => true),
        onActivity: (ev) => {
          if (ev.kind === "notice") notices.push(ev.text);
        },
      },
    });
    expect(a.mcpDecide).toHaveBeenCalledTimes(1);
    expect(notices[0]).toContain("did not reach Hermes");
  });
});

describe("HUP-S4.1 MCP approval card content", () => {
  it("a tool call card names the server and tool and lists the exact arguments", () => {
    const c = mcpRequestCard(CALL_CARD);
    expect(c.kind).toBe("fields");
    if (c.kind !== "fields") return;
    expect(c.summary).toContain("tx_propose");
    expect(c.summary).toContain("node");
    expect(c.rows).toContainEqual({ k: "Argument to", v: "0x52908400098527886E0F7030069857D2E4169EE7" });
    expect(c.rows).toContainEqual({ k: "Argument value_wei", v: "1" });
    expect(c.rows.find((r) => r.k === "Hints")?.v).toContain("not verified");
  });

  it("a page card shows the host first, then the full address and any warnings", () => {
    const c = mcpRequestCard({ ...URL_CARD, warnings: ["this host uses international characters (punycode); check it carefully"] });
    if (c.kind !== "fields") throw new Error("fields card expected");
    expect(c.rows[0]).toEqual({ k: "Opens", v: "auth.example.com" });
    expect(c.rows[1]).toEqual({ k: "Full address", v: URL });
    expect(c.rows.some((r) => r.k === "Warning" && r.v.includes("punycode"))).toBe(true);
    expect(c.summary).toContain("auth.example.com");
  });

  it("an allowed page that the system could not open is reported as such", async () => {
    const a = api([[call(1, "c1", "mcp__web__connect_account")], [result(2, "c1", "ok", "account connected"), done(3)]], [URL_CARD]);
    a.mcpDecide = vi.fn(async (_id: string, approvalId: string, allow: boolean, subject: string) => {
      a.decided.push({ approvalId, allow, subject });
      throw new Error("MCP_PAGE_NOT_OPENED: the page could not be opened (no browser)");
    });
    const notices: string[] = [];
    const p = createSidecarProvider(a, () => "p", () => [], () => null, OPTS);
    await p.send({
      messages: [{ role: "user", content: "connect" }],
      callbacks: { ...cb(async () => true), onActivity: (ev) => ev.kind === "notice" && notices.push(ev.text) },
    });
    expect(notices[0]).toContain("could not be opened (no browser)");
    expect(notices[0]).toContain(URL);
  });
});
