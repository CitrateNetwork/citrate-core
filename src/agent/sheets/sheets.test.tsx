// HUP-S10.2 (US-10.2 AC2) — the Google Sheets view.
//
// BDD:
//   Given no Google OAuth client id, then the view says so and Read/Add stay disabled.
//   Given a connected account, when the member reads a range, then its cells show in a table.
//   When the member adds rows, then exactly the typed rows are sent and the range is read again.
//   A Google error is shown, never hidden. The web preview says the view needs the desktop app.
import { describe, it, expect } from "vitest";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { SheetsPanel, SHEETS_DESKTOP_ONLY, parseRowsText } from "./SheetsPanel";
import type { ScheduleIo } from "../schedule/schedule";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const SHEET = "1AbCdEfGhIjKlMnOpQrStUvWxYz012345";

type Calls = Array<[string, Record<string, unknown>]>;
function makeIo(handler: (cmd: string, a: Record<string, unknown>) => unknown, mode: "tauri" | "sim" = "tauri") {
  const calls: Calls = [];
  const io: ScheduleIo = {
    mode,
    invoke: async <T,>(cmd: string, a: Record<string, unknown>) => {
      calls.push([cmd, a]);
      const r = handler(cmd, a);
      if (r instanceof Error) throw r;
      return r as T;
    },
  };
  return { io, calls };
}

const status = (configured: boolean, connected: boolean) => [
  { service: "gsheets", configured, connected, note: !configured ? "Google is not set up in this build: it needs a Google OAuth client id (see Settings > Connections)" : connected ? null : "not connected to Google yet" },
  { service: "gcal", configured, connected, note: null },
];

async function mount(io: ScheduleIo) {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(<SheetsPanel io={async () => io} />);
  });
  await act(async () => {});
  return { host, root };
}

const $ = (host: HTMLElement, id: string) => host.querySelector(`[data-testid="${id}"]`) as HTMLElement | null;

function type(el: HTMLElement | null, value: string) {
  const input = el as HTMLInputElement | HTMLTextAreaElement;
  const proto = input instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
  Object.getOwnPropertyDescriptor(proto, "value")!.set!.call(input, value);
  input.dispatchEvent(new Event("input", { bubbles: true }));
}

describe("SheetsPanel", () => {
  it("says Google is not set up and keeps Read and Add disabled", async () => {
    const { io, calls } = makeIo((cmd) => (cmd === "google_workspace_status" ? status(false, false) : new Error("unexpected")));
    const { host, root } = await mount(io);
    expect($(host, "sheets-status")!.textContent).toMatch(/needs a Google OAuth client id/);
    expect(($(host, "sheets-read") as HTMLButtonElement).disabled).toBe(true);
    expect(($(host, "sheets-add") as HTMLButtonElement).disabled).toBe(true);
    expect(calls.map((c) => c[0])).toEqual(["google_workspace_status"]);
    act(() => root.unmount());
  });

  it("reads a range from a pasted link and shows the cells", async () => {
    const { io, calls } = makeIo((cmd) =>
      cmd === "google_workspace_status" ? status(true, true) : cmd === "gsheets_read" ? { range: "Budget!A1:B2", rows: [["Item", "Cost"], ["=SUM(A1)", 3]], truncated: false } : new Error("unexpected"),
    );
    const { host, root } = await mount(io);
    await act(async () => {
      type($(host, "sheets-id"), `https://docs.google.com/spreadsheets/d/${SHEET}/edit`);
      type($(host, "sheets-range"), "Budget!A1:B2");
    });
    await act(async () => {
      $(host, "sheets-read")!.click();
    });
    expect(calls.at(-1)).toEqual(["gsheets_read", { spreadsheetId: SHEET, range: "Budget!A1:B2" }]);
    const cells = Array.from(host.querySelectorAll('[data-testid="sheets-table"] td')).map((c) => c.textContent);
    expect(cells).toEqual(["Item", "Cost", "=SUM(A1)", "3"]);
    expect($(host, "sheets-range-read")!.textContent).toBe("Budget!A1:B2 · 2 rows");
    act(() => root.unmount());
  });

  it("adds exactly the typed rows, then reads the range again", async () => {
    const { io, calls } = makeIo((cmd) =>
      cmd === "google_workspace_status"
        ? status(true, true)
        : cmd === "gsheets_append"
          ? { updatedRange: "Budget!A3:B4", updatedRows: 2, updatedCells: 4 }
          : cmd === "gsheets_read"
            ? { range: "Budget!A1:B4", rows: [], truncated: false }
            : new Error("unexpected"),
    );
    const { host, root } = await mount(io);
    await act(async () => {
      type($(host, "sheets-id"), SHEET);
      type($(host, "sheets-range"), "Budget!A:B");
      type($(host, "sheets-rows"), "tea, 2\ncake\t4.5\n\n");
    });
    expect($(host, "sheets-add")!.textContent).toBe("Add 2 rows");
    await act(async () => {
      $(host, "sheets-add")!.click();
    });
    expect(calls.find((c) => c[0] === "gsheets_append")).toEqual(["gsheets_append", { spreadsheetId: SHEET, range: "Budget!A:B", rows: [["tea", "2"], ["cake", "4.5"]] }]);
    expect(calls.at(-1)?.[0]).toBe("gsheets_read");
    expect($(host, "sheets-done")!.textContent).toBe("Added 2 rows at Budget!A3:B4.");
    expect($(host, "sheets-empty")!.textContent).toMatch(/empty/);
    act(() => root.unmount());
  });

  it("shows Google's error", async () => {
    const { io } = makeIo((cmd) => (cmd === "google_workspace_status" ? status(true, true) : new Error("Google could not find that spreadsheet or range")));
    const { host, root } = await mount(io);
    await act(async () => {
      type($(host, "sheets-id"), SHEET);
      type($(host, "sheets-range"), "Nope!A1");
    });
    await act(async () => {
      $(host, "sheets-read")!.click();
    });
    expect($(host, "sheets-error")!.textContent).toBe("Google could not find that spreadsheet or range");
    act(() => root.unmount());
  });

  it("needs the desktop app in the web preview", async () => {
    const { io, calls } = makeIo(() => new Error("no"), "sim");
    const { host, root } = await mount(io);
    expect($(host, "sheets-desktop-only")!.textContent).toBe(SHEETS_DESKTOP_ONLY);
    expect(SHEETS_DESKTOP_ONLY).toMatch(/Google Sheets needs the desktop app/);
    expect(calls).toEqual([]);
    act(() => root.unmount());
  });

  it("splits pasted rows by tab, else by comma, and skips blank lines", () => {
    expect(parseRowsText("a\tb, c\n\n d , e ")).toEqual([["a", "b, c"], ["d", "e"]]);
  });
});
