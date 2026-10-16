// HUP-S10.2 (US-10.2) — Hermes's schedule as a calendar, with the member's Google Calendar beside
// it when connected.
//
// BDD:
//   AC2 Google Calendar via Connections when linked: "shows Google events only when connected",
//       "says Google is not set up when there is no client id".
//   AC3 Hermes's own schedule is visible as a calendar: "lays the week out by day",
//       "adds, pauses and removes Hermes entries".
import { describe, it, expect } from "vitest";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { SchedulePanel } from "./SchedulePanel";
import { clock, itemsForDay, mergeItems, parseLocalDateTime, weekDays, weekStart, type ScheduleIo, type ScheduleView } from "./schedule";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

// A Wednesday, 10:00 local.
const NOW = Math.floor(new Date(2026, 8, 30, 10, 0).getTime() / 1000);
const MON = Math.floor(new Date(2026, 8, 28).getTime() / 1000);
const at = (d: number, h: number, m = 0) => Math.floor(new Date(2026, 8, d, h, m).getTime() / 1000);

function sched(over: Partial<ScheduleView> = {}): ScheduleView {
  return {
    status: "ok",
    error: null,
    entries: [
      { id: "s1", title: "Morning brief", notes: "", start: at(28, 8), durationMins: 15, repeat: "daily", until: null, enabled: true, origin: "member", createdAt: NOW },
      { id: "s2", title: "Weekly review", notes: "", start: at(30, 16), durationMins: 60, repeat: "weekly", until: null, enabled: false, origin: "hermes", createdAt: NOW },
    ],
    occurrences: [
      { entryId: "s1", title: "Morning brief", start: at(28, 8), end: at(28, 8, 15), repeat: "daily", origin: "member", disabled: false },
      { entryId: "s1", title: "Morning brief", start: at(29, 8), end: at(29, 8, 15), repeat: "daily", origin: "member", disabled: false },
      { entryId: "s2", title: "Weekly review", start: at(30, 16), end: at(30, 17), repeat: "weekly", origin: "hermes", disabled: true },
    ],
    ...over,
  };
}

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

const gStatus = (configured: boolean, connected: boolean) => [
  { service: "gsheets", configured, connected, note: configured ? null : "Google is not set up in this build: it needs a Google OAuth client id" },
  {
    service: "gcal",
    configured,
    connected,
    note: !configured ? "Google is not set up in this build: it needs a Google OAuth client id (see Settings > Connections)" : connected ? null : "not connected to Google yet",
  },
];

async function mount(el: React.ReactElement) {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => root.render(el));
  for (let i = 0; i < 4; i++) await act(async () => {});
  return { host, root };
}
const q = (h: HTMLElement, id: string) => h.querySelector(`[data-testid="${id}"]`) as HTMLElement | null;
const qa = (h: HTMLElement, id: string) => Array.from(h.querySelectorAll(`[data-testid="${id}"]`)) as HTMLElement[];
async function click(el: Element | null | undefined) {
  expect(el).toBeTruthy();
  await act(async () => (el as HTMLElement).click());
  for (let i = 0; i < 4; i++) await act(async () => {});
}
async function setValue(el: HTMLInputElement | null, v: string) {
  expect(el).toBeTruthy();
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(el, v);
    el!.dispatchEvent(new Event("input", { bubbles: true }));
  });
}

describe("schedule layout helpers", () => {
  it("weeks start on Monday at local midnight and have seven calendar days", () => {
    expect(weekStart(NOW)).toBe(MON);
    expect(weekStart(MON)).toBe(MON);
    const days = weekDays(MON);
    expect(days).toHaveLength(7);
    expect(new Date(days[6] * 1000).getDate()).toBe(4); // Sun 4 Oct
  });

  it("merges Hermes and Google items by start and places them on the days they overlap", () => {
    const items = mergeItems(sched().occurrences, [{ id: "g1", title: "Dentist", start: at(29, 7), end: at(29, 8), allDay: false, location: "" }]);
    expect(items.map((i) => i.title)).toEqual(["Morning brief", "Dentist", "Morning brief", "Weekly review"]);
    const tue = itemsForDay(items, weekDays(MON)[1], weekDays(MON)[2]);
    expect(tue.map((i) => i.source)).toEqual(["google", "hermes"]);
    expect(clock(at(29, 7, 5))).toBe("07:05");
  });

  it("parses a datetime-local value as local time and refuses anything else", () => {
    expect(parseLocalDateTime("2026-09-30T10:00")).toBe(NOW);
    expect(parseLocalDateTime("2026-09-30")).toBeNull();
    expect(parseLocalDateTime("")).toBeNull();
  });
});

describe("HUP-S10.2 SchedulePanel", () => {
  it("in the web preview says the schedule needs the desktop app", async () => {
    const { io, calls } = makeIo(() => sched(), "sim");
    const { host } = await mount(<SchedulePanel io={async () => io} nowSecs={() => NOW} />);
    expect(q(host, "schedule-desktop-only")?.textContent).toContain("desktop app");
    expect(calls).toEqual([]);
  });

  it("lays the week out by day, with paused entries marked", async () => {
    const { io, calls } = makeIo((cmd) => (cmd === "hermes_schedule_list" ? sched() : cmd === "google_workspace_status" ? gStatus(false, false) : []));
    const { host } = await mount(<SchedulePanel io={async () => io} nowSecs={() => NOW} />);
    const list = calls.find(([c]) => c === "hermes_schedule_list")!;
    expect(list[1].from).toBe(MON);
    const days = qa(host, "schedule-day");
    expect(days).toHaveLength(7);
    expect(days[0].textContent).toContain("08:00 Morning brief");
    expect(days[2].textContent).toContain("16:00 Weekly review (paused)");
    expect(qa(host, "schedule-item-hermes")).toHaveLength(3);
  });

  it("says Google is not set up when there is no client id, and never lists its events", async () => {
    const { io, calls } = makeIo((cmd) => (cmd === "hermes_schedule_list" ? sched() : cmd === "google_workspace_status" ? gStatus(false, false) : []));
    const { host } = await mount(<SchedulePanel io={async () => io} nowSecs={() => NOW} />);
    expect(q(host, "schedule-google")?.textContent).toContain("client id");
    expect(calls.some(([c]) => c === "gcal_list")).toBe(false);
  });

  it("shows Google events only when connected", async () => {
    const { io, calls } = makeIo((cmd) =>
      cmd === "hermes_schedule_list" ? sched({ occurrences: [] }) : cmd === "google_workspace_status" ? gStatus(true, true) : cmd === "gcal_list" ? [{ id: "g1", title: "Dentist", start: at(29, 7), end: at(29, 8), allDay: false, location: "" }] : [],
    );
    const { host } = await mount(<SchedulePanel io={async () => io} nowSecs={() => NOW} />);
    expect(calls.find(([c]) => c === "gcal_list")![1]).toEqual({ from: MON, to: weekDays(MON)[6] + 86_400 });
    expect(qa(host, "schedule-item-google").map((e) => e.textContent)).toEqual(["07:00 Dentist"]);
  });

  it("adds, pauses and removes Hermes entries", async () => {
    const { io, calls } = makeIo((cmd) => (cmd === "hermes_schedule_list" ? sched() : cmd === "google_workspace_status" ? gStatus(false, false) : null));
    const { host } = await mount(<SchedulePanel io={async () => io} nowSecs={() => NOW} />);
    expect((q(host, "schedule-add") as HTMLButtonElement).disabled).toBe(true);
    await setValue(q(host, "schedule-title") as HTMLInputElement, "Check the node");
    await setValue(q(host, "schedule-when") as HTMLInputElement, "2026-09-30T10:00");
    await click(q(host, "schedule-add"));
    const add = calls.find(([c]) => c === "hermes_schedule_add")!;
    expect(add[1]).toEqual({ entry: { title: "Check the node", notes: "", start: NOW, durationMins: 30, repeat: "none", until: null } });
    const entry = qa(host, "schedule-entry")[0];
    await click(entry.querySelectorAll("button")[0]);
    expect(calls.find(([c]) => c === "hermes_schedule_set_enabled")![1]).toEqual({ id: "s1", enabled: false });
    await click(qa(host, "schedule-entry")[0].querySelectorAll("button")[1]);
    expect(calls.find(([c]) => c === "hermes_schedule_remove")![1]).toEqual({ id: "s1" });
  });

  it("a corrupted schedule is shown with a reset, and errors are shown, not swallowed", async () => {
    const { io, calls } = makeIo((cmd) =>
      cmd === "hermes_schedule_list" ? sched({ status: "corrupted", error: "the saved schedule could not be read", entries: [], occurrences: [] }) : cmd === "google_workspace_status" ? gStatus(false, false) : null,
    );
    const { host } = await mount(<SchedulePanel io={async () => io} nowSecs={() => NOW} />);
    expect(q(host, "schedule-corrupted")?.textContent).toContain("could not be read");
    await click(q(host, "schedule-corrupted")?.querySelector("button"));
    expect(calls.some(([c]) => c === "hermes_schedule_reset")).toBe(true);

    const bad = makeIo((cmd) => (cmd === "hermes_schedule_list" ? new Error("disk full") : gStatus(false, false)));
    const m = await mount(<SchedulePanel io={async () => bad.io} nowSecs={() => NOW} />);
    expect(q(m.host, "schedule-error")?.textContent).toContain("disk full");
  });
});
