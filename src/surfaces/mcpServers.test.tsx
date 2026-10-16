// HUP-S4.4 — Settings > MCP servers: add/edit/remove, review before enable, masked env values.
import { describe, it, expect, vi } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { renderToStaticMarkup } from "react-dom/server";
import { McpServersPanel } from "./McpServersPanel";
import {
  draftFromServer,
  draftToInput,
  emptyDraft,
  errorFor,
  toolBadges,
  type McpIo,
  type McpServerView,
  type McpReviewResult,
  type McpProbeTool,
} from "./mcpServers";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const NOTES: McpServerView = {
  name: "notes",
  transport: "stdio",
  command: "/opt/notes/bin/notes-mcp",
  args: ["--root", "/Users/me/notes"],
  cwd: null,
  url: null,
  env: [{ key: "NOTES_TOKEN", masked: "•••••••• (11 characters)" }],
  allowWriteTools: false,
  enabled: false,
  needsReview: true,
};

const TOOL_RO: McpProbeTool = {
  name: "search",
  exposedName: "mcp__notes__search",
  description: "Search notes.",
  annotations: { readOnlyHint: true, destructiveHint: null, idempotentHint: null, openWorldHint: false, title: null },
  effective: { readOnly: true, destructive: false, idempotent: false, openWorld: false },
  trust: "untrusted",
  offered: true,
  skipReason: null,
};
const TOOL_W: McpProbeTool = {
  name: "delete_note",
  exposedName: "mcp__notes__delete_note",
  description: "Delete a note.",
  annotations: { readOnlyHint: false, destructiveHint: true, idempotentHint: null, openWorldHint: null, title: null },
  effective: { readOnly: false, destructive: true, idempotent: false, openWorld: true },
  trust: "untrusted",
  offered: false,
  skipReason: "not annotated read-only, and this server does not allow write tools",
};

const REVIEW: McpReviewResult = {
  errors: [],
  review: {
    server: NOTES,
    commandLine: "/opt/notes/bin/notes-mcp --root /Users/me/notes",
    reviewToken: "a".repeat(64),
    canEnable: true,
    probe: {
      name: "notes",
      transport: "stdio",
      ok: true,
      error: null,
      protocolVersion: "2025-06-18",
      serverName: "notes",
      serverVersion: "1.0.0",
      capabilities: { tools: {} },
      tools: [TOOL_RO, TOOL_W],
      toolsTruncated: false,
      allowWriteTools: false,
    },
  },
};

function makeIo(handler: (cmd: string, args: Record<string, unknown>) => unknown, mode: McpIo["mode"] = "tauri"): { io: McpIo; calls: [string, Record<string, unknown>][] } {
  const calls: [string, Record<string, unknown>][] = [];
  return {
    calls,
    io: {
      mode,
      invoke: async <T,>(cmd: string, args: Record<string, unknown>) => {
        calls.push([cmd, args]);
        return (await handler(cmd, args)) as T;
      },
    },
  };
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
async function type(el: Element | null, value: string) {
  expect(el).toBeTruthy();
  const proto = el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
  const setter = Object.getOwnPropertyDescriptor(proto, "value")?.set;
  await act(async () => {
    setter?.call(el, value);
    el!.dispatchEvent(new Event("input", { bubbles: true }));
  });
}

describe("mcpServers logic", () => {
  it("turns a draft into the save input: one argument per line, kept env values as null", () => {
    const d = draftFromServer(NOTES);
    expect(d.env).toEqual([{ key: "NOTES_TOKEN", value: "", keep: true }]);
    d.argsText = "--root\n/Users/me/notes\n\n";
    const input = draftToInput(d, "notes");
    expect(input).toEqual({
      name: "notes",
      transport: "stdio",
      command: "/opt/notes/bin/notes-mcp",
      args: ["--root", "/Users/me/notes"],
      cwd: null,
      url: null,
      env: [{ key: "NOTES_TOKEN", value: null }],
      allowWriteTools: false,
      previousName: "notes",
    });
  });

  it("an http draft carries only the URL", () => {
    const d = { ...emptyDraft(), name: "scan2", transport: "http" as const, url: "https://scan.example/api/mcp", command: "/x", argsText: "a" };
    const input = draftToInput(d, null);
    expect(input.command).toBeNull();
    expect(input.args).toEqual([]);
    expect(input.env).toEqual([]);
    expect(input.url).toBe("https://scan.example/api/mcp");
    expect(input.previousName).toBeNull();
  });

  it("labels every tool untrusted and names its annotations", () => {
    const ro = toolBadges(TOOL_RO).map((b) => b.label);
    expect(ro).toEqual(expect.arrayContaining(["read-only", "untrusted"]));
    expect(ro).not.toContain("destructive");
    const w = toolBadges(TOOL_W).map((b) => b.label);
    expect(w).toEqual(expect.arrayContaining(["writes", "destructive", "open world", "untrusted"]));
  });

  it("finds the error for a field", () => {
    expect(errorFor([{ field: "env.X", message: "bad" }], "env.X")).toBe("bad");
    expect(errorFor([], "name")).toBeNull();
  });
});

describe("McpServersPanel", () => {
  it("says honestly that the web preview cannot add servers", async () => {
    const { io, calls } = makeIo(() => [], "sim");
    const { host, root } = await mount(<McpServersPanel io={async () => io} />);
    expect(host.textContent).toMatch(/desktop app/);
    expect(calls).toEqual([]);
    root.unmount();
  });

  it("lists servers with env values masked and a needs-review state", async () => {
    const { io } = makeIo((cmd) => (cmd === "mcp_servers_list" ? [NOTES] : null));
    const { host, root } = await mount(<McpServersPanel io={async () => io} />);
    const text = host.textContent ?? "";
    expect(text).toContain("notes");
    expect(text).toContain("/opt/notes/bin/notes-mcp");
    expect(text).toContain("NOTES_TOKEN");
    expect(text).toContain("11 characters");
    expect(text).toMatch(/needs review/i);
    root.unmount();
  });

  it("adds a server: saves it disabled and shows field errors inline", async () => {
    let attempt = 0;
    const { io, calls } = makeIo((cmd) => {
      if (cmd === "mcp_servers_list") return [];
      if (cmd === "mcp_server_save") {
        attempt += 1;
        return attempt === 1
          ? { ok: false, errors: [{ field: "command", message: "use the absolute path to the program (no PATH lookup)" }], servers: [] }
          : { ok: true, errors: [], servers: [NOTES] };
      }
      return null;
    });
    const { host, root } = await mount(<McpServersPanel io={async () => io} />);
    await click(q(host, "mcp-add"));
    await type(q(host, "mcp-name"), "notes");
    await type(q(host, "mcp-command"), "notes-mcp");
    await type(q(host, "mcp-args"), "--root\n/Users/me/notes");
    await click(q(host, "mcp-env-add"));
    await type(q(host, "mcp-env-key-0"), "NOTES_TOKEN");
    await type(q(host, "mcp-env-value-0"), "tok-abc-123");
    expect(q<HTMLInputElement>(host, "mcp-env-value-0")?.type).toBe("password");
    await click(q(host, "mcp-save"));
    expect(q(host, "mcp-err-command")?.textContent).toMatch(/absolute path/);
    await type(q(host, "mcp-command"), "/opt/notes/bin/notes-mcp");
    await click(q(host, "mcp-save"));
    const saves = calls.filter(([c]) => c === "mcp_server_save");
    expect(saves[1][1]).toEqual({
      input: {
        name: "notes",
        transport: "stdio",
        command: "/opt/notes/bin/notes-mcp",
        args: ["--root", "/Users/me/notes"],
        cwd: null,
        url: null,
        env: [{ key: "NOTES_TOKEN", value: "tok-abc-123" }],
        allowWriteTools: false,
        previousName: null,
      },
    });
    // Back on the list, the new server needs review; nothing was enabled.
    expect(host.textContent).toMatch(/needs review/i);
    expect(calls.some(([c]) => c === "mcp_server_enable")).toBe(false);
    root.unmount();
  });

  it("reviews before enabling: exact command line, masked env, tools with annotations, all untrusted", async () => {
    const { io, calls } = makeIo((cmd) => {
      if (cmd === "mcp_servers_list") return [NOTES];
      if (cmd === "mcp_server_review") return REVIEW;
      if (cmd === "mcp_server_enable") return [{ ...NOTES, enabled: true, needsReview: false }];
      return null;
    });
    const { host, root } = await mount(<McpServersPanel io={async () => io} />);
    expect(q(host, "mcp-enable")).toBeNull();
    await click(q(host, "mcp-review-notes"));
    const review = q(host, "mcp-review");
    const text = review?.textContent ?? "";
    expect(q(host, "mcp-review-command")?.textContent).toBe("/opt/notes/bin/notes-mcp --root /Users/me/notes");
    expect(text).toContain("NOTES_TOKEN");
    expect(text).not.toContain("tok-abc-123");
    expect(text).toContain("search");
    expect(text).toContain("delete_note");
    expect(text).toContain("read-only");
    expect(text).toContain("destructive");
    expect(text).toMatch(/not offered/i);
    expect((text.match(/untrusted/g) ?? []).length).toBeGreaterThanOrEqual(2);
    expect(text).toMatch(/next time Hermes starts/);
    await click(q(host, "mcp-enable"));
    expect(calls.find(([c]) => c === "mcp_server_enable")?.[1]).toEqual({ name: "notes", reviewToken: "a".repeat(64) });
    expect(host.textContent).toMatch(/enabled/i);
    root.unmount();
  });

  it("a failed check cannot be enabled and says why", async () => {
    const failed: McpReviewResult = {
      errors: [],
      review: { ...REVIEW.review!, canEnable: false, probe: { ...REVIEW.review!.probe, ok: false, error: "could not start the server: not found", tools: [] } },
    };
    const { io } = makeIo((cmd) => (cmd === "mcp_servers_list" ? [NOTES] : cmd === "mcp_server_review" ? failed : null));
    const { host, root } = await mount(<McpServersPanel io={async () => io} />);
    await click(q(host, "mcp-review-notes"));
    expect(host.textContent).toContain("could not start the server");
    expect(q<HTMLButtonElement>(host, "mcp-enable")?.disabled).toBe(true);
    root.unmount();
  });

  it("shows the Hermes-not-running error from the check", async () => {
    const { io } = makeIo((cmd) => {
      if (cmd === "mcp_servers_list") return [NOTES];
      if (cmd === "mcp_server_review") throw new Error("Start Hermes to check a server");
      return null;
    });
    const { host, root } = await mount(<McpServersPanel io={async () => io} />);
    await click(q(host, "mcp-review-notes"));
    expect(q(host, "mcp-error")?.textContent).toMatch(/Start Hermes/);
    root.unmount();
  });

  it("editing keeps env values unless retyped, and remove asks first", async () => {
    const { io, calls } = makeIo((cmd) => {
      if (cmd === "mcp_servers_list") return [NOTES];
      if (cmd === "mcp_server_save") return { ok: true, errors: [], servers: [NOTES] };
      if (cmd === "mcp_server_remove") return [];
      return null;
    });
    const { host, root } = await mount(<McpServersPanel io={async () => io} />);
    await click(q(host, "mcp-edit-notes"));
    expect(q<HTMLInputElement>(host, "mcp-env-value-0")?.placeholder).toMatch(/keep/i);
    await click(q(host, "mcp-save"));
    const save = calls.find(([c]) => c === "mcp_server_save")?.[1] as { input: { env: unknown; previousName: string } };
    expect(save.input.env).toEqual([{ key: "NOTES_TOKEN", value: null }]);
    expect(save.input.previousName).toBe("notes");
    await click(q(host, "mcp-remove-notes"));
    expect(calls.some(([c]) => c === "mcp_server_remove")).toBe(false);
    await click(q(host, "mcp-remove-confirm-notes"));
    expect(calls.find(([c]) => c === "mcp_server_remove")?.[1]).toEqual({ name: "notes" });
    expect(host.textContent).toMatch(/No MCP servers added/);
    root.unmount();
  });

  it("the static render has no em-dash in member-facing copy", () => {
    const html = renderToStaticMarkup(<McpServersPanel io={async () => makeIo(() => []).io} />);
    expect(html).not.toContain("—");
  });
});

describe("Settings has an MCP servers section", () => {
  it("lists it in the sub-nav", async () => {
    const fs = await import("node:fs");
    const path = await import("node:path");
    const src = fs.readFileSync(path.resolve(process.cwd(), "src/surfaces/Settings.tsx"), "utf8");
    expect(src).toContain('["mcp", "MCP servers"]');
    expect(src).toContain("<McpServersPanel");
  });
});

// vi is imported for parity with the other panel tests' tooling.
void vi;

describe("HUP-S4.1: the live state of an enabled server", () => {
  it("shows connected with the tool count from the running Hermes", async () => {
    const on = { ...NOTES, enabled: true, needsReview: false };
    const { io } = makeIo((cmd) =>
      cmd === "mcp_servers_list"
        ? [on]
        : cmd === "mcp_servers_runtime"
          ? { running: true, configured: true, servers: [{ name: "notes", transport: "stdio", state: "ready", era: "modern", tools: 3, skipped: [] }] }
          : null,
    );
    const { host, root } = await mount(<McpServersPanel io={async () => io} />);
    expect(q(host, "mcp-runtime-notes")?.textContent).toBe("connected · 3 tools · MCP 2026-07-28");
    root.unmount();
  });

  it("shows a failed server with its reason, and nothing for a server still under review", async () => {
    const on = { ...NOTES, enabled: true, needsReview: false };
    const off = { ...NOTES, name: "draft", enabled: false };
    const { io } = makeIo((cmd) =>
      cmd === "mcp_servers_list"
        ? [on, off]
        : cmd === "mcp_servers_runtime"
          ? { running: true, servers: [{ name: "notes", transport: "stdio", state: "failed", tools: 0, skipped: [], error: "could not start the server", nextRetryMs: 2000 }] }
          : null,
    );
    const { host, root } = await mount(<McpServersPanel io={async () => io} />);
    expect(q(host, "mcp-runtime-notes")?.textContent).toBe("failed: could not start the server · retrying in 2 s");
    expect(q(host, "mcp-runtime-draft")).toBeNull();
    root.unmount();
  });
});
