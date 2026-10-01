// HUP-S4.2 + S8.5 — Settings panel for the citrate-node MCP server.
import { describe, it, expect, vi } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { NodeMcpPanel } from "./NodeMcpPanel";
import {
  DESKTOP_ONLY,
  ago,
  claudeHttpCommand,
  claudeStdioCommand,
  shellQuote,
  stateLabel,
  type NodeMcpIo,
  type NodeMcpRequest,
  type NodeMcpStatus,
} from "./nodeMcp";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const TOKEN = "cnmcp_" + "ab".repeat(32);

function baseStatus(over: Partial<NodeMcpStatus> = {}): NodeMcpStatus {
  return {
    running: false,
    enabled: false,
    port: 47204,
    endpoint: "http://127.0.0.1:47204/mcp",
    lastError: null,
    tokens: [],
    pendingRequests: 0,
    recentCalls: [],
    stdioCommand: "/Applications/Citrate Core.app/Contents/MacOS/citrate-core",
    ...over,
  };
}

const sigReq = (over: Partial<NodeMcpRequest> = {}): NodeMcpRequest => ({
  id: "mcpr-1",
  tokenId: "abcd1234",
  origin: "mcp:Claude Code via claude-code",
  summary: "Sign and send a transaction: Transfer",
  kind: "signature",
  ceremony_id: "7",
  ceremony: { id: "7", origin: "mcp:Claude Code via claude-code", decoded: { action: "Transfer", cost: "1 SALT", destination: "0x02" }, requiresRawAck: false },
  state: "pending",
  createdMs: 1,
  decidedMs: null,
  ...over,
});

function makeIo(handlers: Record<string, (args: Record<string, unknown>) => unknown>) {
  const invoke = vi.fn(async (cmd: string, args: Record<string, unknown>) => {
    const h = handlers[cmd];
    if (!h) throw new Error("unexpected " + cmd);
    return h(args);
  });
  const io: NodeMcpIo = { mode: "tauri", invoke: invoke as NodeMcpIo["invoke"] };
  return { io, invoke };
}

async function mount(el: React.ReactElement): Promise<{ host: HTMLDivElement; root: Root }> {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(el);
  });
  await act(async () => {});
  return { host, root };
}
const q = <T extends Element = HTMLElement>(host: HTMLElement, id: string) => host.querySelector(`[data-testid="${id}"]`) as T | null;
async function click(el: Element | null) {
  expect(el).toBeTruthy();
  await act(async () => {
    (el as HTMLElement).click();
  });
  await act(async () => {});
}

describe("connect-command builders", () => {
  it("builds a Claude Code HTTP command with the bearer header", () => {
    expect(claudeHttpCommand("http://127.0.0.1:47204/mcp", TOKEN)).toBe(
      `claude mcp add --transport http citrate-node http://127.0.0.1:47204/mcp --header 'Authorization: Bearer ${TOKEN}'`,
    );
  });
  it("builds a stdio shim command, quoting a path with spaces and adding the port only when it is not the default", () => {
    const exe = "/Applications/Citrate Core.app/Contents/MacOS/citrate-core";
    expect(claudeStdioCommand(exe, TOKEN, 47204)).toBe(
      `claude mcp add citrate-node -e CITRATE_NODE_MCP_TOKEN=${TOKEN} -- '${exe}' --mcp-stdio`,
    );
    expect(claudeStdioCommand(exe, TOKEN, 5000)).toContain("-e CITRATE_NODE_MCP_PORT=5000");
  });
  it("quotes shell arguments safely", () => {
    expect(shellQuote("plain-arg")).toBe("plain-arg");
    expect(shellQuote("it's here")).toBe(`'it'\\''s here'`);
    expect(shellQuote("$(rm -rf ~)")).toBe("'$(rm -rf ~)'");
  });
  it("labels request states and times honestly", () => {
    expect(stateLabel(sigReq())).toBe("waiting for you");
    expect(stateLabel(sigReq({ state: "failed", error: "insufficient funds" }))).toBe("failed: insufficient funds");
    expect(stateLabel(sigReq({ state: "expired" }))).toBe("expired before a decision");
    expect(ago(null, 10)).toBe("never");
    expect(ago(1_000, 61_000)).toBe("1m ago");
  });
});

describe("NodeMcpPanel", () => {
  it("says it is desktop only in the web preview and calls nothing", async () => {
    const invoke = vi.fn();
    const { host } = await mount(<NodeMcpPanel io={async () => ({ mode: "sim", invoke })} pollMs={0} />);
    expect(q(host, "nodemcp-desktop-only")?.textContent).toBe(DESKTOP_ONLY);
    expect(invoke).not.toHaveBeenCalled();
  });

  it("shows the server off by default and turns it on through node_mcp_set_enabled", async () => {
    const { io, invoke } = makeIo({
      node_mcp_status: () => baseStatus(),
      node_mcp_requests: () => [],
      node_mcp_set_enabled: (a) => baseStatus({ running: !!a.enabled, enabled: !!a.enabled }),
    });
    const { host } = await mount(<NodeMcpPanel io={async () => io} pollMs={0} />);
    expect(q(host, "nodemcp-state")?.textContent).toBe("Off");
    expect(host.textContent).toContain("No tokens yet. No client can connect.");
    await click(q(host, "nodemcp-toggle"));
    expect(invoke).toHaveBeenCalledWith("node_mcp_set_enabled", { enabled: true });
  });

  it("shows a new token once with ready-made commands, then hides it", async () => {
    const { io, invoke } = makeIo({
      node_mcp_status: () => baseStatus({ running: true }),
      node_mcp_requests: () => [],
      node_mcp_token_create: (a) => ({ id: "abcd1234", label: a.label, createdMs: 1, connectToken: TOKEN }),
    });
    const { host } = await mount(<NodeMcpPanel io={async () => io} pollMs={0} />);
    await click(q(host, "nodemcp-create"));
    expect(invoke).toHaveBeenCalledWith("node_mcp_token_create", { label: "Claude Code" });
    expect(q(host, "nodemcp-token")?.textContent).toBe(TOKEN);
    expect(q(host, "nodemcp-cmd-http")?.textContent).toContain("--transport http citrate-node");
    expect(q(host, "nodemcp-cmd-stdio")?.textContent).toContain("--mcp-stdio");
    expect(host.textContent).toContain("shown once");
    await click(q(host, "nodemcp-issued-done"));
    expect(q(host, "nodemcp-token")).toBeNull();
    expect(host.textContent).not.toContain(TOKEN);
  });

  it("lists tokens without secrets and revokes one", async () => {
    const { io, invoke } = makeIo({
      node_mcp_status: () => baseStatus({ tokens: [{ id: "abcd1234", label: "Cursor", createdMs: 1, lastUsedMs: null }] }),
      node_mcp_requests: () => [],
      node_mcp_token_revoke: () => true,
    });
    const { host } = await mount(<NodeMcpPanel io={async () => io} pollMs={0} />);
    expect(q(host, "nodemcp-token-abcd1234")?.textContent).toContain("Cursor");
    expect(host.textContent).not.toContain("cnmcp_");
    await click(q(host, "nodemcp-revoke-abcd1234"));
    expect(invoke).toHaveBeenCalledWith("node_mcp_token_revoke", { id: "abcd1234" });
  });

  it("shows a pending transaction with its decoded action and approves it through node_mcp_decide", async () => {
    const { io, invoke } = makeIo({
      node_mcp_status: () => baseStatus({ running: true, pendingRequests: 1 }),
      node_mcp_requests: () => [sigReq()],
      node_mcp_decide: (a) => sigReq({ state: a.approve ? "approved" : "rejected" }),
    });
    const { host } = await mount(<NodeMcpPanel io={async () => io} pollMs={0} />);
    const card = q(host, "nodemcp-req-mcpr-1");
    expect(card?.textContent).toContain("from mcp:Claude Code via claude-code");
    expect(card?.textContent).toContain("action: Transfer");
    expect(card?.textContent).toContain("cost: 1 SALT");
    await click(q(host, "nodemcp-approve-mcpr-1"));
    expect(invoke).toHaveBeenCalledWith("node_mcp_decide", { id: "mcpr-1", approve: true, rawAck: false });
  });

  it("will not approve an undecodable transaction until the raw-data box is ticked", async () => {
    const req = sigReq({ ceremony: { id: "7", origin: "mcp:x", decoded: { action: "Unrecognized", cost: "", destination: "" }, requiresRawAck: true } });
    const { io, invoke } = makeIo({
      node_mcp_status: () => baseStatus({ running: true }),
      node_mcp_requests: () => [req],
      node_mcp_decide: () => req,
    });
    const { host } = await mount(<NodeMcpPanel io={async () => io} pollMs={0} />);
    const approve = q<HTMLButtonElement>(host, "nodemcp-approve-mcpr-1");
    expect(approve?.disabled).toBe(true);
    await click(q(host, "nodemcp-rawack-mcpr-1"));
    expect(q<HTMLButtonElement>(host, "nodemcp-approve-mcpr-1")?.disabled).toBe(false);
    await click(q(host, "nodemcp-approve-mcpr-1"));
    expect(invoke).toHaveBeenCalledWith("node_mcp_decide", { id: "mcpr-1", approve: true, rawAck: true });
  });

  it("rejects a cluster action and surfaces command errors", async () => {
    const action: NodeMcpRequest = {
      ...sigReq({ kind: "action", ceremony: undefined, ceremony_id: undefined, summary: "Join this node to the cluster of group grp_a" }),
      action: { action: "cluster_join", group: "grp_a" },
    };
    const { io, invoke } = makeIo({
      node_mcp_status: () => baseStatus({ running: true }),
      node_mcp_requests: () => [action],
      node_mcp_decide: () => {
        throw new Error("request mcpr-1 is no longer waiting for a decision");
      },
    });
    const { host } = await mount(<NodeMcpPanel io={async () => io} pollMs={0} />);
    expect(q(host, "nodemcp-approve-mcpr-1")?.textContent).toBe("Approve");
    await click(q(host, "nodemcp-reject-mcpr-1"));
    expect(invoke).toHaveBeenCalledWith("node_mcp_decide", { id: "mcpr-1", approve: false, rawAck: false });
    expect(q(host, "nodemcp-error")?.textContent).toContain("no longer waiting");
  });

  it("shows a bind failure from the server honestly", async () => {
    const { io } = makeIo({
      node_mcp_status: () => baseStatus({ lastError: "could not listen on 127.0.0.1:47204: Address already in use" }),
      node_mcp_requests: () => [],
    });
    const { host } = await mount(<NodeMcpPanel io={async () => io} pollMs={0} />);
    expect(q(host, "nodemcp-last-error")?.textContent).toContain("Address already in use");
  });
});
