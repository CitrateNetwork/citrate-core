// =====================================================================
// HUP-S10.2 — Hermes's schedule and the member's Google Calendar (webview half).
//
// The schedule lives in Rust (src-tauri/src/hermes_schedule.rs, core's app data). Google Calendar
// comes through Connections (src-tauri/src/google_workspace.rs) and stays off until a Google
// OAuth client id is configured and the member connects. This module is the typed seam plus the
// pure week layout the calendar view uses.
// =====================================================================

export type Repeat = "none" | "daily" | "weekly";

export interface ScheduleEntry {
  id: string;
  title: string;
  notes: string;
  start: number;
  durationMins: number;
  repeat: Repeat;
  until: number | null;
  enabled: boolean;
  origin: "member" | "hermes";
  createdAt: number;
}

export interface Occurrence {
  entryId: string;
  title: string;
  start: number;
  end: number;
  repeat: Repeat;
  origin: "member" | "hermes";
  disabled: boolean;
}

export interface ScheduleView {
  status: "ok" | "corrupted" | string;
  error: string | null;
  entries: ScheduleEntry[];
  occurrences: Occurrence[];
}

export interface GoogleServiceStatus {
  service: "gsheets" | "gcal" | string;
  configured: boolean;
  connected: boolean;
  note: string | null;
}

export interface CalendarEvent {
  id: string;
  title: string;
  start: number;
  end: number;
  allDay: boolean;
  location: string;
}

/** One item on the calendar: a Hermes occurrence or a Google event. */
export interface CalendarItem {
  key: string;
  source: "hermes" | "google";
  title: string;
  start: number;
  end: number;
  allDay: boolean;
  disabled: boolean;
  entryId: string | null;
}

export interface ScheduleIo {
  mode: "tauri" | "sim";
  invoke<T>(cmd: string, args: Record<string, unknown>): Promise<T>;
}

export const SCHEDULE_DESKTOP_ONLY = "Hermes's schedule needs the desktop app. The web preview keeps no schedule.";

const DAY = 86_400;

/** Local midnight (unix seconds) of the Monday on or before `secs`. */
export function weekStart(secs: number): number {
  const d = new Date(secs * 1000);
  d.setHours(0, 0, 0, 0);
  const dow = (d.getDay() + 6) % 7; // Monday = 0
  d.setDate(d.getDate() - dow);
  return Math.floor(d.getTime() / 1000);
}

/** The 7 local-midnight day starts of the week beginning at `start` (DST-safe: by calendar date). */
export function weekDays(start: number): number[] {
  const out: number[] = [];
  const d = new Date(start * 1000);
  for (let i = 0; i < 7; i++) {
    const x = new Date(d.getFullYear(), d.getMonth(), d.getDate() + i);
    out.push(Math.floor(x.getTime() / 1000));
  }
  return out;
}

/** Merge Hermes occurrences and Google events into calendar items, sorted by start. */
export function mergeItems(occ: Occurrence[], events: CalendarEvent[]): CalendarItem[] {
  const items: CalendarItem[] = [
    ...occ.map((o) => ({
      key: `h:${o.entryId}:${o.start}`,
      source: "hermes" as const,
      title: o.title,
      start: o.start,
      end: o.end,
      allDay: false,
      disabled: o.disabled,
      entryId: o.entryId,
    })),
    ...events.map((e) => ({
      key: `g:${e.id}:${e.start}`,
      source: "google" as const,
      title: e.title,
      start: e.start,
      end: e.end,
      allDay: e.allDay,
      disabled: false,
      entryId: null,
    })),
  ];
  return items.sort((a, b) => a.start - b.start || a.key.localeCompare(b.key));
}

/** The items that overlap the day starting at `dayStart` (until the next day's start). */
export function itemsForDay(items: CalendarItem[], dayStart: number, nextDayStart: number = dayStart + DAY): CalendarItem[] {
  return items.filter((i) => i.start < nextDayStart && i.end > dayStart);
}

/** `HH:MM` local. */
export function clock(secs: number): string {
  const d = new Date(secs * 1000);
  return `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
}

/** Parse a `datetime-local` value (`YYYY-MM-DDTHH:MM`, local time) to unix seconds, or null. */
export function parseLocalDateTime(v: string): number | null {
  const m = /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2})$/.exec(v);
  if (!m) return null;
  const d = new Date(+m[1], +m[2] - 1, +m[3], +m[4], +m[5]);
  const t = d.getTime();
  return Number.isFinite(t) ? Math.floor(t / 1000) : null;
}

export function errorMessage(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

export async function desktopScheduleIo(): Promise<ScheduleIo> {
  const { BRIDGE_MODE } = await import("../../bridge/mode");
  return {
    mode: BRIDGE_MODE,
    invoke: async <T,>(cmd: string, args: Record<string, unknown>) => {
      const { invoke } = await import("../../bridge/tauri/invoke");
      return invoke<T>(cmd, args);
    },
  };
}
