// =====================================================================
// HUP-S10.2 (US-10.2 AC2) — the Google Sheets view (Journal > Sheets).
//
// The member reads a range of one of their Google spreadsheets and adds rows to it, through the
// Rust commands gsheets_read / gsheets_append (src-tauri/src/google_workspace.rs, Google's Sheets
// v4 API with the member's Connections token). Nothing here asks Hermes: these are the member's
// own actions. Hermes reaches the same commands through its gsheets_read / gsheets_append tools,
// where an append waits for the member's approval card (src/agent/everydayTools.ts).
//
// Honest states (Rule 1): with no Google OAuth client id configured, or before the member
// connects, the view says so and keeps Read and Add disabled. Appended values are stored as typed
// (valueInputOption=RAW), so a cell that starts with "=" stays text and no formula runs.
// =====================================================================
import { useCallback, useEffect, useState } from "react";
import { errorMessage, type GoogleServiceStatus, type ScheduleIo } from "../schedule/schedule";
import { spreadsheetIdOf } from "../everydayTools";

export interface SheetsPanelProps {
  io: () => Promise<ScheduleIo>;
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

export const SHEETS_DESKTOP_ONLY = "Google Sheets needs the desktop app. The web preview cannot reach Google.";
/** Most rows the view shows at once (Rust returns at most 2,000). */
export const VIEW_ROWS = 200;
/** Most rows one Add sends from this view. */
export const MAX_FORM_ROWS = 100;

/** The rows typed in the Add box: one row per line, cells split by tabs (pasted from a sheet) or commas. */
export function parseRowsText(text: string): string[][] {
  return text
    .split(/\r?\n/)
    .map((l) => l.trimEnd())
    .filter((l) => l.trim().length > 0)
    .map((l) => (l.includes("\t") ? l.split("\t") : l.split(",")).map((c) => c.trim()));
}

function cellText(v: unknown): string {
  if (v === null || v === undefined) return "";
  return typeof v === "string" ? v : JSON.stringify(v);
}

const note = { fontSize: 12, color: "var(--tx-2)", lineHeight: 1.5 };
const row = { display: "flex", gap: 8, alignItems: "flex-end", flexWrap: "wrap" as const };

export function SheetsPanel({ io }: SheetsPanelProps) {
  const [x, setX] = useState<ScheduleIo | null>(null);
  const [status, setStatus] = useState<GoogleServiceStatus | null>(null);
  const [statusErr, setStatusErr] = useState<string | null>(null);
  const [sheet, setSheet] = useState("");
  const [range, setRange] = useState("");
  const [values, setValues] = useState<SheetValues | null>(null);
  const [rowsText, setRowsText] = useState("");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [done, setDone] = useState<string | null>(null);

  const loadStatus = useCallback(async (m: ScheduleIo) => {
    try {
      const st = await m.invoke<GoogleServiceStatus[]>("google_workspace_status", {});
      setStatus(st.find((s) => s.service === "gsheets") ?? null);
      setStatusErr(null);
    } catch (e) {
      setStatusErr(errorMessage(e));
    }
  }, []);

  useEffect(() => {
    let live = true;
    void io().then((m) => {
      if (!live) return;
      setX(m);
      if (m.mode === "tauri") void loadStatus(m);
    });
    return () => {
      live = false;
    };
  }, [io, loadStatus]);

  if (x && x.mode !== "tauri") {
    return (
      <div data-testid="sheets-desktop-only" style={note}>
        {SHEETS_DESKTOP_ONLY}
      </div>
    );
  }

  const ready = !!x && !!status?.connected;
  const id = spreadsheetIdOf(sheet);
  const rows = parseRowsText(rowsText);
  const canRead = ready && !busy && id.length > 0 && range.trim().length > 0;
  const canAdd = canRead && rows.length > 0 && rows.length <= MAX_FORM_ROWS;

  const run = async (fn: (m: ScheduleIo) => Promise<void>) => {
    if (!x) return;
    setBusy(true);
    setErr(null);
    setDone(null);
    try {
      await fn(x);
    } catch (e) {
      setErr(errorMessage(e));
    } finally {
      setBusy(false);
    }
  };
  const read = () =>
    run(async (m) => {
      setValues(await m.invoke<SheetValues>("gsheets_read", { spreadsheetId: id, range: range.trim() }));
    });
  const add = () =>
    run(async (m) => {
      const out = await m.invoke<AppendResult>("gsheets_append", { spreadsheetId: id, range: range.trim(), rows });
      setDone(`Added ${out.updatedRows} row${out.updatedRows === 1 ? "" : "s"} at ${out.updatedRange || range.trim()}.`);
      setRowsText("");
      setValues(await m.invoke<SheetValues>("gsheets_read", { spreadsheetId: id, range: range.trim() }));
    });

  const shown = values ? values.rows.slice(0, VIEW_ROWS) : [];
  const width = shown.reduce((w, r) => Math.max(w, r.length), 0);

  return (
    <div data-testid="sheets-panel" style={{ display: "flex", flexDirection: "column", gap: 10 }}>
      <span data-testid="sheets-status" style={note}>
        Google Sheets:{" "}
        {status === null ? statusErr ?? "status unknown" : status.connected ? "connected" : status.note ?? "not connected"}
      </span>
      <div style={row}>
        <label style={{ ...note, flex: "2 1 220px" }}>
          Spreadsheet link or id
          <input data-testid="sheets-id" className="input" value={sheet} onChange={(e) => setSheet(e.target.value)} disabled={!ready} />
        </label>
        <label style={{ ...note, flex: "1 1 140px" }}>
          Range
          <input data-testid="sheets-range" className="input" placeholder="Sheet1!A1:D20" value={range} onChange={(e) => setRange(e.target.value)} disabled={!ready} />
        </label>
        <button data-testid="sheets-read" className="btn btn-sm" disabled={!canRead} onClick={() => void read()}>
          Read
        </button>
      </div>

      {values && (
        <div style={{ overflowX: "auto" }}>
          <span data-testid="sheets-range-read" style={note}>
            {values.range || range} · {values.rows.length} row{values.rows.length === 1 ? "" : "s"}
            {values.rows.length > VIEW_ROWS || values.truncated ? ` (showing the first ${Math.min(VIEW_ROWS, values.rows.length)})` : ""}
          </span>
          {shown.length === 0 ? (
            <div data-testid="sheets-empty" style={note}>
              That range is empty.
            </div>
          ) : (
            <table data-testid="sheets-table" style={{ borderCollapse: "collapse", fontSize: 12 }}>
              <caption style={{ ...note, textAlign: "left" }}>Cells of {values.range || range}</caption>
              <tbody>
                {shown.map((r, i) => (
                  <tr key={i}>
                    {Array.from({ length: width }, (_, j) => (
                      <td key={j} style={{ border: "1px solid var(--line-1)", padding: "3px 6px", overflowWrap: "anywhere" }}>
                        {cellText(r[j])}
                      </td>
                    ))}
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </div>
      )}

      <label style={note}>
        Rows to add (one per line; cells separated by tabs or commas)
        <textarea data-testid="sheets-rows" className="input" rows={3} value={rowsText} onChange={(e) => setRowsText(e.target.value)} disabled={!ready} />
      </label>
      <div style={row}>
        <button data-testid="sheets-add" className="btn btn-sm" disabled={!canAdd} onClick={() => void add()}>
          Add {rows.length || ""} row{rows.length === 1 ? "" : "s"}
        </button>
        <span style={note}>
          Values are stored as typed, so nothing you add runs as a formula. At most {MAX_FORM_ROWS} rows at a time. Remove rows in Google Sheets itself.
        </span>
      </div>
      {done && (
        <div role="status" data-testid="sheets-done" style={note}>
          {done}
        </div>
      )}
      {err && (
        <div role="alert" data-testid="sheets-error" style={{ fontSize: 12, color: "var(--danger)" }}>
          {err}
        </div>
      )}
    </div>
  );
}
