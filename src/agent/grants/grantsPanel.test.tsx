// HUP-S2.1 — the Grants panel (Settings): list grants with their countdown, grant a folder with
// read and write as separate toggles, revoke, the read-only 24 h full-access window behind a
// HIC-1 confirmation with its own countdown, and the honest corrupted-file state.
//
// BDD (US-2.1):
//   AC1 read and write are separate: "grants read only unless write is also ticked", "write alone".
//   AC3 full access is read-only, expires in 24 h, countdown shown: the full-access scenarios.
//   Corrupted store grants nothing and the panel says so: "a corrupted store".
import { describe, it, expect, vi } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { renderToStaticMarkup } from "react-dom/server";
import { GrantsPanel } from "./GrantsPanel";
import { fmtCountdown, type GrantsChange, type GrantsIo, type GrantsView, type FullAccessConfirmation } from "./grants";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const NOW = 1_790_000_000;
const HOME = "/Users/member";

function view(over: Partial<GrantsView> = {}): GrantsView {
  return { status: "ok", error: null, grants: [], fullAccessRemainingSecs: null, fullAccessRoot: HOME, now: NOW, ...over };
}
function change(v: GrantsView, sync = { updated: 0, failed: [] as string[] }): GrantsChange {
  return { view: v, sync };
}
const folderRow = (id: string, access: "read" | "write", status = "active") => ({
  id,
  kind: "folder",
  root: HOME + "/work/app",
  access,
  status,
  grantedAt: NOW - 60,
  expiresAt: null,
  remainingSecs: null,
  reason: "Granted in Settings",
});
const fullRow = (remaining: number, status = "active") => ({
  id: "g-9",
  kind: "full_access",
  root: HOME,
  access: "read" as const,
  status,
  grantedAt: NOW - 10,
  expiresAt: NOW + remaining,
  remainingSecs: remaining,
  reason: "Read-only full access (24 h), confirmed in Settings",
});

type Calls = Array<[string, Record<string, unknown>]>;
function makeIo(handler: (cmd: string, args: Record<string, unknown>) => unknown, pick: string | null = HOME + "/work/app", mode: "tauri" | "sim" = "tauri") {
  const calls: Calls = [];
  const io: GrantsIo = {
    mode,
    pickFolder: vi.fn(async () => pick),
    invoke: async <T,>(cmd: string, args: Record<string, unknown>) => {
      calls.push([cmd, args]);
      const r = handler(cmd, args);
      if (r instanceof Error) throw r;
      return r as T;
    },
  };
  return { io, calls };
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

describe("fmtCountdown", () => {
  it("formats hours, minutes and seconds without rounding up", () => {
    expect(fmtCountdown(86_400)).toBe("24 h 00 m");
    expect(fmtCountdown(3_599)).toBe("59 m 59 s");
    expect(fmtCountdown(61)).toBe("1 m 01 s");
    expect(fmtCountdown(0)).toBe("0 s");
    expect(fmtCountdown(-5)).toBe("0 s");
  });
});

describe("GrantsPanel", () => {
  it("in the web preview says grants are desktop-only and offers no controls", async () => {
    const { io, calls } = makeIo(() => view(), null, "sim");
    const { host } = await mount(<GrantsPanel io={async () => io} nowSecs={() => NOW} />);
    expect(host.textContent).toContain("desktop app");
    expect(q(host, "grant-folder")).toBeNull();
    expect(calls).toEqual([]);
  });

  it("lists grants with their access and status, and an empty state when there are none", async () => {
    const empty = makeIo(() => view());
    let m = await mount(<GrantsPanel io={async () => empty.io} nowSecs={() => NOW} />);
    expect(m.host.textContent).toContain("No folders are granted");
    const { io } = makeIo(() => view({ grants: [folderRow("g-1", "read"), folderRow("g-2", "write", "revoked")] }));
    m = await mount(<GrantsPanel io={async () => io} nowSecs={() => NOW} />);
    const rows = m.host.querySelectorAll('[data-testid="grant-row"]');
    expect(rows.length).toBe(2);
    expect(rows[0].textContent).toContain("/Users/member/work/app");
    expect(rows[0].textContent).toContain("read");
    expect(rows[1].textContent).toContain("revoked");
    // Only live grants can be revoked.
    expect(rows[0].querySelector('[data-testid="revoke"]')).toBeTruthy();
    expect(rows[1].querySelector('[data-testid="revoke"]')).toBeNull();
  });

  it("grants read only unless write is also ticked", async () => {
    const { io, calls } = makeIo((cmd) => (cmd === "agent_grants_view" ? view() : change(view({ grants: [folderRow("g-1", "read")] }), { updated: 1, failed: [] })));
    const { host } = await mount(<GrantsPanel io={async () => io} nowSecs={() => NOW} />);
    expect(q<HTMLInputElement>(host, "grant-read")!.checked).toBe(true);
    expect(q<HTMLInputElement>(host, "grant-write")!.checked).toBe(false);
    await click(q(host, "grant-folder"));
    expect(calls.find(([c]) => c === "agent_grants_add_folder")).toEqual(["agent_grants_add_folder", { path: HOME + "/work/app", read: true, write: false }]);
    expect(host.textContent).toContain("1 open Hermes conversation");
  });

  it("can grant write alone, and refuses neither", async () => {
    const { io, calls } = makeIo((cmd) => (cmd === "agent_grants_view" ? view() : change(view())));
    const { host } = await mount(<GrantsPanel io={async () => io} nowSecs={() => NOW} />);
    await click(q(host, "grant-read"));
    await click(q(host, "grant-folder"));
    expect(calls.some(([c]) => c === "agent_grants_add_folder")).toBe(false);
    expect(host.textContent).toContain("Choose read, write, or both");
    await click(q(host, "grant-write"));
    await click(q(host, "grant-folder"));
    expect(calls.find(([c]) => c === "agent_grants_add_folder")![1]).toEqual({ path: HOME + "/work/app", read: false, write: true });
  });

  it("a cancelled folder picker changes nothing", async () => {
    const { io, calls } = makeIo(() => view(), null);
    const { host } = await mount(<GrantsPanel io={async () => io} nowSecs={() => NOW} />);
    await click(q(host, "grant-folder"));
    expect(calls.map(([c]) => c)).toEqual(["agent_grants_view"]);
  });

  it("shows a refusal from core as it is", async () => {
    const { io } = makeIo((cmd) => (cmd === "agent_grants_view" ? view() : new Error("/Users/member/.ssh holds credentials and cannot be granted")));
    const { host } = await mount(<GrantsPanel io={async () => io} nowSecs={() => NOW} />);
    await click(q(host, "grant-folder"));
    expect(host.textContent).toContain("holds credentials");
  });

  it("revokes a grant by its id and reports sessions that could not take the change", async () => {
    const { io, calls } = makeIo((cmd) =>
      cmd === "agent_grants_view"
        ? view({ grants: [folderRow("g-1", "read")] })
        : change(view({ grants: [folderRow("g-1", "read", "revoked")] }), { updated: 0, failed: ["a Hermes conversation could not be reached (refused)"] }),
    );
    const { host } = await mount(<GrantsPanel io={async () => io} nowSecs={() => NOW} />);
    await click(q(host, "revoke"));
    expect(calls.find(([c]) => c === "agent_grants_revoke")).toEqual(["agent_grants_revoke", { id: "g-1" }]);
    expect(host.textContent).toContain("revoked");
    expect(host.textContent).toContain("could not be reached");
  });

  it("full access needs the HIC-1 confirmation: prepare shows the exact statement and a countdown, confirm sends that id", async () => {
    const conf: FullAccessConfirmation = {
      id: "c0ffee",
      root: HOME,
      statement: "For the next 24 hours Hermes may read any file under /Users/member. It cannot change, create or delete anything through this window.",
      preparedAt: NOW,
      confirmBy: NOW + 120,
      grantExpiresAt: NOW + 86_400,
    };
    const { io, calls } = makeIo((cmd) => {
      if (cmd === "agent_grants_view") return view();
      if (cmd === "agent_grants_full_access_prepare") return conf;
      return change(view({ grants: [fullRow(86_400)], fullAccessRemainingSecs: 86_400 }), { updated: 1, failed: [] });
    });
    const { host } = await mount(<GrantsPanel io={async () => io} nowSecs={() => NOW + 20} />);
    // Nothing is granted by the first click.
    await click(q(host, "full-access-on"));
    expect(calls.some(([c]) => c === "agent_grants_full_access_confirm")).toBe(false);
    const card = q(host, "full-access-confirm-card")!;
    expect(card.textContent).toContain(conf.statement);
    expect(card.textContent).toContain("1 m 40 s");
    expect(card.textContent).toContain("read-only");
    await click(q(host, "full-access-confirm"));
    expect(calls.find(([c]) => c === "agent_grants_full_access_confirm")).toEqual(["agent_grants_full_access_confirm", { id: "c0ffee" }]);
    expect(q(host, "full-access-confirm-card")).toBeNull();
    expect(q(host, "full-access-countdown")!.textContent).toContain("24 h 00 m");
  });

  it("cancelling the confirmation grants nothing", async () => {
    const { io, calls } = makeIo((cmd) =>
      cmd === "agent_grants_view" ? view() : { id: "x", root: HOME, statement: "s", preparedAt: NOW, confirmBy: NOW + 120, grantExpiresAt: NOW + 86_400 },
    );
    const { host } = await mount(<GrantsPanel io={async () => io} nowSecs={() => NOW} />);
    await click(q(host, "full-access-on"));
    await click(q(host, "full-access-cancel"));
    expect(q(host, "full-access-confirm-card")).toBeNull();
    expect(calls.some(([c]) => c === "agent_grants_full_access_confirm")).toBe(false);
  });

  it("an expired confirmation cannot be confirmed", async () => {
    const { io, calls } = makeIo((cmd) =>
      cmd === "agent_grants_view" ? view() : { id: "x", root: HOME, statement: "s", preparedAt: NOW, confirmBy: NOW + 120, grantExpiresAt: NOW + 86_400 },
    );
    const { host } = await mount(<GrantsPanel io={async () => io} nowSecs={() => NOW + 121} />);
    await click(q(host, "full-access-on"));
    expect(q<HTMLButtonElement>(host, "full-access-confirm")!.disabled).toBe(true);
    expect(host.textContent).toContain("timed out");
    expect(calls.some(([c]) => c === "agent_grants_full_access_confirm")).toBe(false);
  });

  it("while full access is on it shows the countdown and turns off by revoking that grant", async () => {
    const { io, calls } = makeIo((cmd) =>
      cmd === "agent_grants_view" ? view({ grants: [fullRow(3_600)], fullAccessRemainingSecs: 3_600 }) : change(view({ grants: [fullRow(3_600, "revoked")] })),
    );
    const { host } = await mount(<GrantsPanel io={async () => io} nowSecs={() => NOW} />);
    expect(q(host, "full-access-countdown")!.textContent).toContain("1 h 00 m");
    expect(q(host, "full-access-on")).toBeNull();
    await click(q(host, "full-access-off"));
    expect(calls.find(([c]) => c === "agent_grants_revoke")).toEqual(["agent_grants_revoke", { id: "g-9" }]);
  });

  it("a corrupted store grants nothing, says so, and offers a reset instead of grant controls", async () => {
    const { io, calls } = makeIo((cmd) =>
      cmd === "agent_grants_view"
        ? view({ status: "corrupted", error: "The saved folder grants could not be read (not a grant document). Hermes is treated as having no folder access, so it grants nothing until you reset them." })
        : change(view()),
    );
    const { host } = await mount(<GrantsPanel io={async () => io} nowSecs={() => NOW} />);
    expect(host.textContent).toContain("grants nothing");
    expect(q(host, "grant-folder")).toBeNull();
    expect(q(host, "full-access-on")).toBeNull();
    await click(q(host, "grants-reset"));
    expect(calls.map(([c]) => c)).toContain("agent_grants_reset");
    expect(q(host, "grant-folder")).toBeTruthy();
  });

  it("server-renders without an em-dash and names HIC, never HITL", () => {
    const { io } = makeIo(() => view());
    const html = renderToStaticMarkup(<GrantsPanel io={async () => io} nowSecs={() => NOW} />);
    expect(html).not.toContain("—");
    expect(html).not.toContain("HITL");
  });
});
