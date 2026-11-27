// L6-WP01 (AR-E8.2, D8): a connected GitHub, Drive or Notion sign-in is sealed in the vault, but no
// agent tool uses it yet. The surfaces said "mounted as agent tool" and suffixed every Settings row
// with " · MCP tool". Until an MCP gateway reports a connector's tools as live, the row says the
// connection is not yet available to the agent, and no copy claims otherwise.
import { afterEach, describe, expect, it, vi } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { renderToStaticMarkup } from "react-dom/server";
import { Connections } from "./Connections";
import { Settings } from "./Settings";
import { bridge } from "../bridge";
import { store } from "../shell/store";
import { freshState } from "../shell/state";
import type { Store } from "../shell/store";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement | null = null;
let root: Root | null = null;
afterEach(() => {
  act(() => root?.unmount());
  host?.remove();
  host = null;
  root = null;
  vi.restoreAllMocks();
});

async function mountConnected() {
  vi.spyOn(bridge.connections, "status").mockResolvedValue([
    { service: "github", connected: true, scope: "repo read:org", connectedAt: 1_700_000_000 },
    { service: "acme", connected: true, scope: "acme files", connectedAt: 1_700_000_000 },
  ]);
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
  await act(async () => {
    root?.render(<Connections store={store} s={freshState("p1")} />);
  });
  // let the status() promise resolve and the rows re-render
  await act(async () => {
    await Promise.resolve();
  });
}

const settingsStore = {
  identity: () => ({ name: "Member", initials: "M", email: "m@example.com", sub: "sub-1", wallet: "0x", tier: "pilot", role: "member", org: null, real: true }),
} as unknown as Store;

describe("L6-WP01 Connections never claims an agent tool it does not have", () => {
  it("never_renders_mounted_as_agent_tool_without_gateway_report", async () => {
    await mountConnected();
    const text = host!.textContent ?? "";
    // The rows really are connected (the Disconnect action is offered for both).
    const disconnect = [...host!.querySelectorAll("button")].filter((b) => b.textContent === "Disconnect");
    expect(disconnect.length).toBe(2);
    expect(text).not.toMatch(/mounted as agent tool/i);
    // The honest note, once per connected row (GitHub and the extra service).
    const notes = text.match(/Connected\. Not yet available to your agent/g) ?? [];
    expect(notes.length).toBe(2);
    expect(text).not.toMatch(/Connected\. Available to your agent/);
  });

  it("settings_rows_do_not_claim_mcp_tool", () => {
    const s = freshState("p1");
    s.sSec = "connections";
    const html = renderToStaticMarkup(<Settings store={settingsStore} s={s} />);
    expect(html).toContain("GitHub");
    expect(html).not.toContain(" · MCP tool");
    expect(html).not.toMatch(/mount as MCP tools/i);
  });

  it("mcp_footer_does_not_claim_tools_mount", () => {
    const html = renderToStaticMarkup(<Connections store={{} as unknown as Store} s={freshState("p1")} />);
    expect(html).not.toMatch(/connected servers mount as tools/i);
    // It says where tokens live and which connectors the agent can use today.
    expect(html).toMatch(/sealed on this computer/i);
    expect(html).toMatch(/your agent can use Google Sheets and Google Calendar today/i);
  });
});
