// =====================================================================
// HUP-S10.2 — the Schedule panel (Journal > Schedule), US-10.2 AC2 + AC3.
//
// Hermes's own schedule as a week calendar, next to the member's Google Calendar when it is
// connected (Google stays off until a Google OAuth client id is configured; the panel says so).
// The member adds, pauses and removes Hermes entries here, and can add an event to Google.
// Nothing here runs anything: the scheduler (HUP-S10.3) reads what is due.
// =====================================================================
import { useCallback, useEffect, useMemo, useState } from "react";
import {
  SCHEDULE_DESKTOP_ONLY,
  clock,
  errorMessage,
  itemsForDay,
  mergeItems,
  parseLocalDateTime,
  weekDays,
  weekStart,
  type CalendarEvent,
  type GoogleServiceStatus,
  type Repeat,
  type ScheduleIo,
  type ScheduleView,
} from "./schedule";

export interface SchedulePanelProps {
  io: () => Promise<ScheduleIo>;
  /** Unix seconds (injected for tests). */
  nowSecs?: () => number;
}

const realNow = () => Math.floor(Date.now() / 1000);
const note = { fontSize: 12, color: "var(--tx-2)", lineHeight: 1.5 };
const row = { display: "flex", gap: 8, alignItems: "center", flexWrap: "wrap" as const };
const DAY_NAMES = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

export function SchedulePanel({ io, nowSecs = realNow }: SchedulePanelProps) {
  const [x, setX] = useState<ScheduleIo | null>(null);
  const [week, setWeek] = useState(() => weekStart(nowSecs()));
  const [view, setView] = useState<ScheduleView | null>(null);
  const [gcal, setGcal] = useState<GoogleServiceStatus | null>(null);
  const [events, setEvents] = useState<CalendarEvent[]>([]);
  const [gErr, setGErr] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [title, setTitle] = useState("");
  const [when, setWhen] = useState("");
  const [mins, setMins] = useState(30);
  const [repeat, setRepeat] = useState<Repeat>("none");
  const [toGoogle, setToGoogle] = useState(false);
  const days = useMemo(() => weekDays(week), [week]);
  const weekEnd = useMemo(() => {
    const last = new Date(days[6] * 1000);
    return Math.floor(new Date(last.getFullYear(), last.getMonth(), last.getDate() + 1).getTime() / 1000);
  }, [days]);

  const load = useCallback(
    async (m: ScheduleIo) => {
      try {
        setView(await m.invoke<ScheduleView>("hermes_schedule_list", { from: week, to: weekEnd }));
        setErr(null);
      } catch (e) {
        setErr(errorMessage(e));
      }
      let status: GoogleServiceStatus | null = null;
      try {
        const st = await m.invoke<GoogleServiceStatus[]>("google_workspace_status", {});
        status = st.find((s) => s.service === "gcal") ?? null;
      } catch (e) {
        setGErr(errorMessage(e));
      }
      setGcal(status);
      if (status?.connected) {
        try {
          setEvents(await m.invoke<CalendarEvent[]>("gcal_list", { from: week, to: weekEnd }));
          setGErr(null);
        } catch (e) {
          setEvents([]);
          setGErr(errorMessage(e));
        }
      } else {
        setEvents([]);
      }
    },
    [week, weekEnd],
  );

  useEffect(() => {
    let live = true;
    void io().then((m) => {
      if (!live) return;
      setX(m);
      if (m.mode === "tauri") void load(m);
    });
    return () => {
      live = false;
    };
  }, [io, load]);

  if (x && x.mode !== "tauri") {
    return (
      <div data-testid="schedule-desktop-only" style={note}>
        {SCHEDULE_DESKTOP_ONLY}
      </div>
    );
  }

  const items = mergeItems(view?.occurrences ?? [], events);
  const act = async (fn: () => Promise<unknown>) => {
    if (!x) return;
    setErr(null);
    try {
      await fn();
      await load(x);
    } catch (e) {
      setErr(errorMessage(e));
    }
  };
  const start = parseLocalDateTime(when);
  const canAdd = !!x && title.trim().length > 0 && start !== null && mins > 0;
  const add = () =>
    act(async () => {
      if (!x || start === null) return;
      if (toGoogle) {
        await x.invoke("gcal_create", { title, start, end: start + mins * 60, description: "" });
      } else {
        await x.invoke("hermes_schedule_add", { entry: { title, notes: "", start, durationMins: mins, repeat, until: null } });
      }
      setTitle("");
    });

  return (
    <div data-testid="schedule-panel" style={{ display: "flex", flexDirection: "column", gap: 10 }}>
      <div style={row}>
        <button className="btn btn-ghost btn-sm" aria-label="Previous week" onClick={() => setWeek(weekStart(week - 3 * 86_400))}>
          ‹
        </button>
        <span data-testid="schedule-week" style={{ fontSize: 13 }}>
          Week of {new Date(week * 1000).toLocaleDateString()}
        </span>
        <button className="btn btn-ghost btn-sm" aria-label="Next week" onClick={() => setWeek(weekStart(weekEnd + 3600))}>
          ›
        </button>
        <button className="btn btn-ghost btn-sm" onClick={() => setWeek(weekStart(nowSecs()))}>
          This week
        </button>
      </div>

      <span data-testid="schedule-google" style={note}>
        Google Calendar:{" "}
        {gcal === null
          ? gErr ?? "status unknown"
          : gcal.connected
            ? gErr ?? "connected; its events are shown here"
            : gcal.note ?? "not connected"}
      </span>

      {view?.status === "corrupted" && (
        <div role="alert" data-testid="schedule-corrupted" style={{ ...note, color: "var(--danger)" }}>
          {view.error}{" "}
          <button className="btn btn-sm" onClick={() => void act(() => x!.invoke("hermes_schedule_reset", {}))}>
            Set it aside and start empty
          </button>
        </div>
      )}

      <div role="grid" aria-label="Week calendar" style={{ display: "grid", gridTemplateColumns: "repeat(7, minmax(0,1fr))", gap: 6 }}>
        {days.map((d, i) => {
          const next = i < 6 ? days[i + 1] : weekEnd;
          const day = itemsForDay(items, d, next);
          return (
            <div key={d} role="gridcell" data-testid="schedule-day" style={{ border: "1px solid var(--line-1)", borderRadius: 6, padding: 6, minHeight: 80, display: "flex", flexDirection: "column", gap: 4 }}>
              <span className="mono" style={{ fontSize: 10, color: "var(--tx-3)" }}>
                {DAY_NAMES[i]} {new Date(d * 1000).getDate()}
              </span>
              {day.map((it) => (
                <span
                  key={it.key}
                  data-testid={it.source === "hermes" ? "schedule-item-hermes" : "schedule-item-google"}
                  title={it.source === "hermes" ? "On Hermes's schedule" : "From your Google Calendar"}
                  style={{ fontSize: 11, lineHeight: 1.35, opacity: it.disabled ? 0.5 : 1, borderLeft: "2px solid " + (it.source === "hermes" ? "var(--accent)" : "var(--tx-3)"), paddingLeft: 4, overflowWrap: "anywhere" }}
                >
                  {it.allDay ? "all day" : clock(it.start)} {it.title}
                  {it.disabled ? " (paused)" : ""}
                </span>
              ))}
            </div>
          );
        })}
      </div>

      {view && view.entries.length > 0 && (
        <ul aria-label="Hermes schedule entries" style={{ listStyle: "none", margin: 0, padding: 0, display: "flex", flexDirection: "column", gap: 4 }}>
          {view.entries.map((e) => (
            <li key={e.id} data-testid="schedule-entry" style={{ ...row, fontSize: 12 }}>
              <span style={{ flex: 1, minWidth: 0, overflowWrap: "anywhere" }}>
                {e.title} · {new Date(e.start * 1000).toLocaleString()} · {e.repeat === "none" ? "once" : e.repeat} · {e.durationMins} min
                {e.origin === "hermes" ? " · added by Hermes" : ""}
                {e.enabled ? "" : " · paused"}
              </span>
              <button className="btn btn-ghost btn-sm" onClick={() => void act(() => x!.invoke("hermes_schedule_set_enabled", { id: e.id, enabled: !e.enabled }))}>
                {e.enabled ? "Pause" : "Resume"}
              </button>
              <button className="btn btn-ghost btn-sm" onClick={() => void act(() => x!.invoke("hermes_schedule_remove", { id: e.id }))}>
                Remove
              </button>
            </li>
          ))}
        </ul>
      )}

      <div style={{ ...row, alignItems: "flex-end" }}>
        <label style={note}>
          What
          <input data-testid="schedule-title" className="input" value={title} onChange={(e) => setTitle(e.target.value)} />
        </label>
        <label style={note}>
          When
          <input data-testid="schedule-when" className="input" type="datetime-local" value={when} onChange={(e) => setWhen(e.target.value)} />
        </label>
        <label style={note}>
          Minutes
          <input className="input" type="number" min={1} max={10080} value={mins} onChange={(e) => setMins(Math.max(0, Math.floor(Number(e.target.value) || 0)))} style={{ width: 80 }} />
        </label>
        {!toGoogle && (
          <label style={note}>
            Repeat
            <select value={repeat} onChange={(e) => setRepeat(e.target.value as Repeat)}>
              <option value="none">once</option>
              <option value="daily">daily</option>
              <option value="weekly">weekly</option>
            </select>
          </label>
        )}
        <label style={note} title={gcal?.connected ? "" : gcal?.note ?? "Google Calendar is not connected"}>
          <input type="checkbox" checked={toGoogle} disabled={!gcal?.connected} onChange={(e) => setToGoogle(e.target.checked)} /> Add to Google Calendar instead
        </label>
        <button data-testid="schedule-add" className="btn btn-sm" disabled={!canAdd} onClick={() => void add()}>
          Add
        </button>
      </div>
      <span style={note}>Repeats step by exactly a day or a week, so across a daylight saving change the local time of a repeat moves by an hour.</span>
      {err && (
        <div role="alert" data-testid="schedule-error" style={{ fontSize: 12, color: "var(--danger)" }}>
          {err}
        </div>
      )}
    </div>
  );
}
