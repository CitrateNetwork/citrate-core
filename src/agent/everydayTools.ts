// =====================================================================
// HUP-S10.2 (US-10.2 AC2 + AC3) — Hermes's everyday tools: Google Sheets, Hermes's schedule and
// the member's Google Calendar, as chat tools.
//
//   gsheets_read   read a range of a Google spreadsheet            (read, no approval)
//   gsheets_append add rows to a Google spreadsheet                (write, HIC-1 approval card)
//   schedule_list  Hermes's schedule for the next days             (read, no approval)
//   schedule_add   add an entry to Hermes's schedule               (write, HIC-1 approval card)
//   calendar_list  the member's Google Calendar for the next days  (read, no approval)
//
// Data sources (Rule 7): the Rust commands in src-tauri/src/google_workspace.rs (gsheets_read,
// gsheets_append, gcal_list, google_workspace_status; Google's Sheets v4 and Calendar v3 APIs
// through the member's Connections token) and src-tauri/src/hermes_schedule.rs
// (hermes_schedule_list, hermes_schedule_add; core's local schedule file).
//
// Honesty (Rule 1): with no Google OAuth client id configured, or before the member connects,
// every Google tool answers "not connected" from google_workspace_status and touches nothing.
// Text that other people can write (cells of a shared sheet, event titles from invitations) is
// handed to the model fenced as untrusted data. A write runs only after the member's Approve.
// Desktop only: the web preview has no Rust side and says so.
// =====================================================================
import type { ApprovalCard } from "./approvalCards";
import { fieldsCard } from "./approvalCards";
import type { ToolAnnotation } from "./toolAnnotations";
import { fenceUntrusted } from "./untrusted";
import type { CerSpec } from "../shell/state";
import type { CalendarEvent, GoogleServiceStatus, Repeat, ScheduleEntry, ScheduleView } from "./schedule/schedule";

export type EverydayInvoke = <T>(cmd: string, args: Record<string, unknown>) => Promise<T>;

export interface EverydayDeps {
  /** The Rust command seam; null in the web preview. */
  invoke: EverydayInvoke | null;
  /** Ask the member (the SignatureCeremony queue). Resolves "approved" only on an explicit Approve. */
  ask: (spec: CerSpec, card: ApprovalCard) => Promise<string>;
  /** Unix seconds now (injected for tests). */
  nowSecs: () => number;
}

export interface EverydayOutcome {
  /** "ok" for reads and finished writes, "approved"/"declined" when the member decided. */
  status: string;
  result: string;
}

export const EVERYDAY_TOOL_NAMES = ["gsheets_read", "gsheets_append", "schedule_list", "schedule_add", "calendar_list"] as const;
export type EverydayToolName = (typeof EVERYDAY_TOOL_NAMES)[number];

export function isEverydayTool(name: string): name is EverydayToolName {
  return (EVERYDAY_TOOL_NAMES as readonly string[]).includes(name);
}

export const EVERYDAY_DESKTOP_ONLY = "Google Sheets, Google Calendar and Hermes's schedule need the desktop app; nothing was read or changed.";

/** Most days a listing tool looks ahead. */
export const MAX_LIST_DAYS = 31;
/** Most rows gsheets_append accepts from Hermes (Rust allows 500; a chat proposal stays small). */
export const MAX_AGENT_APPEND_ROWS = 50;
/** Most rows and characters of a sheet read handed back to the model. */
export const MAX_READ_ROWS_TO_MODEL = 200;
export const MAX_READ_CHARS_TO_MODEL = 12_000;

const DAY = 86_400;

function str(v: unknown): string {
  return typeof v === "string" ? v.trim() : "";
}

/** A spreadsheet id, or the id inside a Google Sheets link. Rust validates it again. */
export function spreadsheetIdOf(v: unknown): string {
  const s = str(v);
  const m = /\/spreadsheets\/d\/([A-Za-z0-9_-]+)/.exec(s);
  return m ? m[1] : s;
}

/** Rows from the model: a list of lists, or the same as a JSON string. Null when not that shape. */
export function rowsOf(v: unknown): unknown[][] | null {
  let x = v;
  if (typeof x === "string") {
    try {
      x = JSON.parse(x);
    } catch {
      return null;
    }
  }
  if (!Array.isArray(x) || x.length === 0) return null;
  if (!x.every((r) => Array.isArray(r))) return null;
  return x as unknown[][];
}

/** A day count from the model: a whole number from 1 to MAX_LIST_DAYS, default 7. */
export function daysOf(v: unknown): number | null {
  if (v === undefined || v === null || v === "") return 7;
  const n = typeof v === "number" ? v : Number(v);
  if (!Number.isInteger(n) || n < 1 || n > MAX_LIST_DAYS) return null;
  return n;
}

/**
 * A start time from the model: `YYYY-MM-DDTHH:MM` (local time on this computer) or an RFC 3339
 * date-time with `Z` or an offset. Null when it is neither.
 */
export function startOf(v: unknown): number | null {
  const s = str(v);
  const local = /^(\d{4})-(\d{2})-(\d{2})[T ](\d{2}):(\d{2})$/.exec(s);
  if (local) {
    const d = new Date(+local[1], +local[2] - 1, +local[3], +local[4], +local[5]);
    const t = d.getTime();
    return Number.isFinite(t) && d.getMonth() === +local[2] - 1 ? Math.floor(t / 1000) : null;
  }
  if (!/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}(:\d{2}(\.\d+)?)?(Z|[+-]\d{2}:\d{2})$/.test(s)) return null;
  const t = Date.parse(s);
  return Number.isFinite(t) ? Math.floor(t / 1000) : null;
}

function repeatOf(v: unknown): Repeat | null {
  const s = str(v) || "none";
  return s === "none" || s === "daily" || s === "weekly" ? s : null;
}

function errText(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

function when(secs: number): string {
  return new Date(secs * 1000).toISOString().replace(".000Z", "Z");
}

/** The Google service's state, or the honest reason it cannot be used. */
async function googleReady(inv: EverydayInvoke, service: "gsheets" | "gcal"): Promise<string | null> {
  let list: GoogleServiceStatus[];
  try {
    list = await inv<GoogleServiceStatus[]>("google_workspace_status", {});
  } catch (e) {
    return "Google's status could not be read: " + errText(e);
  }
  const st = list.find((s) => s.service === service);
  if (!st) return "This build does not offer that Google service.";
  if (!st.configured || !st.connected) return st.note ?? "Google is not connected.";
  return null;
}

interface SheetValues {
  range: string;
  rows: unknown[][];
  truncated: boolean;
}

interface AppendResult {
  updatedRange: string;
  updatedRows: number;
  updatedCells: number;
}

/** A sheet read as text for the model: rows as JSON, bounded, fenced (shared sheets hold other people's text). */
export function formatSheetForAgent(v: SheetValues): string {
  const rows = v.rows.slice(0, MAX_READ_ROWS_TO_MODEL);
  let body = JSON.stringify(rows);
  let clipped = rows.length < v.rows.length || v.truncated;
  if (body.length > MAX_READ_CHARS_TO_MODEL) {
    body = body.slice(0, MAX_READ_CHARS_TO_MODEL);
    clipped = true;
  }
  const head = `Read ${v.rows.length} row${v.rows.length === 1 ? "" : "s"} from ${v.range || "the range"}${clipped ? " (only the first part is shown)" : ""}.`;
  return head + "\n" + fenceUntrusted("Google Sheets cells", body);
}

async function gsheetsRead(inv: EverydayInvoke, args: Record<string, unknown>): Promise<EverydayOutcome> {
  const id = spreadsheetIdOf(args.spreadsheetId);
  const range = str(args.range);
  if (!id || !range) return { status: "ok", result: "gsheets_read needs a spreadsheetId (or the sheet's link) and an A1 range such as Sheet1!A1:D20. Nothing was read." };
  const why = await googleReady(inv, "gsheets");
  if (why) return { status: "ok", result: "Google Sheets is not available: " + why + " Nothing was read." };
  try {
    return { status: "ok", result: formatSheetForAgent(await inv<SheetValues>("gsheets_read", { spreadsheetId: id, range })) };
  } catch (e) {
    return { status: "ok", result: "couldn't read the sheet: " + errText(e) };
  }
}

async function gsheetsAppend(d: EverydayDeps, inv: EverydayInvoke, args: Record<string, unknown>, ann: ToolAnnotation | null): Promise<EverydayOutcome> {
  const id = spreadsheetIdOf(args.spreadsheetId);
  const range = str(args.range);
  const rows = rowsOf(args.rows);
  if (!id || !range || !rows) {
    return { status: "ok", result: "gsheets_append needs a spreadsheetId, an A1 range and rows (a list of rows, each a list of cells). Nothing was added." };
  }
  if (rows.length > MAX_AGENT_APPEND_ROWS) {
    return { status: "ok", result: `Hermes can propose at most ${MAX_AGENT_APPEND_ROWS} rows at a time; nothing was added.` };
  }
  const why = await googleReady(inv, "gsheets");
  if (why) return { status: "ok", result: "Google Sheets is not available: " + why + " Nothing was added." };
  const cells = rows.reduce((n, r) => n + r.length, 0);
  const r = await d.ask(
    {
      origin: "chat agent",
      requester: "dashboard agent · tool gsheets_append",
      title: "Add rows to a Google spreadsheet",
      chainless: true,
      rows: [
        { k: "Spreadsheet", v: id },
        { k: "Range", v: range },
        { k: "Adds", v: `${rows.length} row${rows.length === 1 ? "" : "s"}, ${cells} cell${cells === 1 ? "" : "s"}, stored as typed (no formulas run)` },
        { k: "Undo", v: "not from this app: remove the rows in Google Sheets" },
      ],
      cost: "none, a Google Sheets write",
      sponsor: "no chain transaction",
      sponsorColor: "var(--tx-3)",
    },
    fieldsCard("gsheets_append", ann, { spreadsheetId: id, range, rows }, `add ${rows.length} row${rows.length === 1 ? "" : "s"} to ${range}`),
  );
  if (r !== "approved") return { status: r, result: "The member declined; nothing was added to the sheet." };
  try {
    const out = await inv<AppendResult>("gsheets_append", { spreadsheetId: id, range, rows });
    return { status: r, result: `Added ${out.updatedRows} row${out.updatedRows === 1 ? "" : "s"} (${out.updatedCells} cells) at ${out.updatedRange || range} with the member's approval.` };
  } catch (e) {
    return { status: r, result: "The member approved, but Google refused the rows: " + errText(e) };
  }
}

/** Hermes's schedule as text for the model. Written by the member or approved by them: trusted. */
export function formatScheduleForAgent(v: ScheduleView, days: number): string {
  if (v.status !== "ok") return "Hermes's schedule could not be read: " + (v.error ?? v.status);
  if (v.occurrences.length === 0) return `Hermes's schedule has nothing in the next ${days} day${days === 1 ? "" : "s"}.`;
  const lines = v.occurrences.slice(0, 100).map((o) => `- ${when(o.start)} to ${when(o.end)}: ${o.title}${o.repeat !== "none" ? ` (repeats ${o.repeat})` : ""}${o.disabled ? " (paused)" : ""}`);
  return `Hermes's schedule, next ${days} day${days === 1 ? "" : "s"} (UTC times):\n` + lines.join("\n");
}

async function scheduleList(d: EverydayDeps, inv: EverydayInvoke, args: Record<string, unknown>): Promise<EverydayOutcome> {
  const days = daysOf(args.days);
  if (days === null) return { status: "ok", result: `schedule_list takes days as a whole number from 1 to ${MAX_LIST_DAYS}. Nothing was read.` };
  const from = d.nowSecs();
  try {
    return { status: "ok", result: formatScheduleForAgent(await inv<ScheduleView>("hermes_schedule_list", { from, to: from + days * DAY }), days) };
  } catch (e) {
    return { status: "ok", result: "couldn't read Hermes's schedule: " + errText(e) };
  }
}

async function scheduleAdd(d: EverydayDeps, inv: EverydayInvoke, args: Record<string, unknown>, ann: ToolAnnotation | null): Promise<EverydayOutcome> {
  const title = str(args.title);
  const start = startOf(args.start);
  const mins = args.durationMins === undefined || args.durationMins === "" ? 30 : Number(args.durationMins);
  const repeat = repeatOf(args.repeat);
  const notes = str(args.notes);
  if (!title || start === null || !Number.isInteger(mins) || mins < 1 || mins > 7 * 24 * 60 || repeat === null) {
    return {
      status: "ok",
      result: "schedule_add needs a title, a start (YYYY-MM-DDTHH:MM local time, or a date-time with a time zone), durationMins from 1 to 10080 and repeat none, daily or weekly. Nothing was added.",
    };
  }
  if (start < d.nowSecs() - 60) return { status: "ok", result: "That start time is in the past; nothing was added." };
  const r = await d.ask(
    {
      origin: "chat agent",
      requester: "dashboard agent · tool schedule_add",
      title: "Add to Hermes's schedule",
      chainless: true,
      rows: [
        { k: "Entry", v: "“" + title + "”" },
        { k: "Starts", v: new Date(start * 1000).toLocaleString() },
        { k: "Length", v: `${mins} minutes` },
        { k: "Repeats", v: repeat },
        { k: "Store", v: "local schedule on this device; remove or pause it in Journal > Schedule" },
      ],
      cost: "none, a local file",
      sponsor: "no chain transaction",
      sponsorColor: "var(--tx-3)",
    },
    fieldsCard("schedule_add", ann, { title, start: when(start), durationMins: mins, repeat, ...(notes ? { notes } : {}) }, `add “${title}” to Hermes's schedule`),
  );
  if (r !== "approved") return { status: r, result: `The member declined; “${title}” was not added to the schedule.` };
  try {
    const entry = await inv<ScheduleEntry>("hermes_schedule_add", {
      entry: { title, notes: notes ? "Proposed by Hermes, approved by you. " + notes : "Proposed by Hermes, approved by you.", start, durationMins: mins, repeat, until: null },
    });
    return { status: r, result: `Added “${entry.title}” to Hermes's schedule at ${when(entry.start)} with the member's approval.` };
  } catch (e) {
    return { status: r, result: "The member approved, but the schedule refused the entry: " + errText(e) };
  }
}

/** Calendar events as text for the model, fenced (invitations carry other people's titles). */
export function formatEventsForAgent(events: CalendarEvent[], days: number): string {
  if (events.length === 0) return `The member's Google Calendar has no events in the next ${days} day${days === 1 ? "" : "s"}.`;
  const rows = events.slice(0, 100).map((e) => ({ start: when(e.start), end: when(e.end), allDay: e.allDay, title: e.title, location: e.location }));
  return `${events.length} event${events.length === 1 ? "" : "s"} in the next ${days} day${days === 1 ? "" : "s"} (UTC times):\n` + fenceUntrusted("Google Calendar events", rows);
}

async function calendarList(d: EverydayDeps, inv: EverydayInvoke, args: Record<string, unknown>): Promise<EverydayOutcome> {
  const days = daysOf(args.days);
  if (days === null) return { status: "ok", result: `calendar_list takes days as a whole number from 1 to ${MAX_LIST_DAYS}. Nothing was read.` };
  const why = await googleReady(inv, "gcal");
  if (why) return { status: "ok", result: "Google Calendar is not available: " + why + " Nothing was read." };
  const from = d.nowSecs();
  try {
    return { status: "ok", result: formatEventsForAgent(await inv<CalendarEvent[]>("gcal_list", { from, to: from + days * DAY }), days) };
  } catch (e) {
    return { status: "ok", result: "couldn't read the calendar: " + errText(e) };
  }
}

/** Run one everyday tool call. Reads never ask; writes ask first and run only on Approve. */
export async function runEverydayTool(
  name: EverydayToolName,
  args: Record<string, unknown>,
  ann: ToolAnnotation | null,
  d: EverydayDeps,
): Promise<EverydayOutcome> {
  const inv = d.invoke;
  if (!inv) return { status: "ok", result: EVERYDAY_DESKTOP_ONLY };
  switch (name) {
    case "gsheets_read":
      return gsheetsRead(inv, args);
    case "gsheets_append":
      return gsheetsAppend(d, inv, args, ann);
    case "schedule_list":
      return scheduleList(d, inv, args);
    case "schedule_add":
      return scheduleAdd(d, inv, args, ann);
    case "calendar_list":
      return calendarList(d, inv, args);
  }
}
