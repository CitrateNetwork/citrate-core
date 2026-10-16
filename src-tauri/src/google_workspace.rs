//! HUP-S10.2 (US-10.2 AC2) — Google Sheets and Google Calendar, through the Connections flow.
//!
//! Both services ride the existing W4 Connections machinery (`connections.rs`): loopback PKCE in
//! the system browser, the access token sealed in the custody vault, and no command that returns a
//! token. This module adds the two API clients on top:
//!
//! * **Sheets** (`gsheets`, scope `spreadsheets`): read a range, append rows. Appends use
//!   `valueInputOption=RAW`, so Google stores every value as typed and never runs a formula.
//! * **Calendar** (`gcal`, scope `calendar.events`): list events of the primary calendar in a
//!   window, create one event.
//!
//! **Disabled until configured.** Like every Connections service, a flow needs a Google OAuth
//! client id and secret (the `GOOGLE_CLIENT_ID` / `GOOGLE_CLIENT_SECRET` pair the Drive service
//! already uses). Until one is configured, [`google_workspace_status`] says so and the UI keeps
//! Connect disabled with that reason. Until the member connects, every call here answers "not
//! connected" without touching the network. An expired token is never sent (the member is asked
//! to reconnect; this build has no refresh-token flow).
//!
//! Writes (append, create) are member actions in the app: Journal > Schedule lists Calendar
//! events and adds one, and Journal > Sheets reads a range and adds rows. Hermes reaches the same
//! commands through its chat tools (src/agent/everydayTools.ts): `gsheets_read` and
//! `calendar_list` read, and `gsheets_append` runs only after the member approves its card.
//!
//! Data sources (Rule 7): `https://sheets.googleapis.com/v4/spreadsheets/{id}/values/{range}` and
//! `https://www.googleapis.com/calendar/v3/calendars/primary/events`.
//!
//! Keyless: nothing here signs or holds a wallet key (Rule 3).

use serde::Serialize;
use serde_json::{json, Value};

use crate::connections::Service;
use crate::oidc::HttpClient;

pub const SHEETS_API: &str = "https://sheets.googleapis.com/v4/spreadsheets";
pub const CALENDAR_EVENTS_API: &str =
    "https://www.googleapis.com/calendar/v3/calendars/primary/events";
// Conservative caps, pending owner sign-off.
/// Most rows one append sends.
pub const MAX_APPEND_ROWS: usize = 500;
/// Most columns per appended row.
pub const MAX_APPEND_COLS: usize = 100;
/// Longest appended cell text, in characters (Google's own cell limit is 50,000).
pub const MAX_CELL_CHARS: usize = 8_192;
/// Most rows a read returns.
pub const MAX_READ_ROWS: usize = 2_000;
/// Most events a listing returns (one page).
pub const MAX_EVENTS: usize = 250;
/// Widest calendar window, in days.
pub const MAX_WINDOW_DAYS: u64 = 92;
/// Longest event title, in characters.
pub const MAX_TITLE_CHARS: usize = 300;
/// Longest event description, in characters.
pub const MAX_DESCRIPTION_CHARS: usize = 4_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GwError {
    NotConfigured,
    NotConnected,
    /// Google answered 401: the sealed token is no longer accepted.
    Expired,
    /// Google answered 403: the connection lacks access to this item.
    Forbidden,
    NotFound,
    Rejected(u16),
    Network,
    BadResponse,
    Invalid(String),
}

impl std::fmt::Display for GwError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GwError::NotConfigured => f.write_str(
                "Google is not set up in this build: it needs a Google OAuth client id (see Settings > Connections)",
            ),
            GwError::NotConnected => {
                f.write_str("not connected to Google yet; connect it in Settings > Connections")
            }
            GwError::Expired => f.write_str(
                "the Google sign-in has expired or was revoked; reconnect it in Settings > Connections",
            ),
            GwError::Forbidden => {
                f.write_str("Google refused access to that item for this connection")
            }
            GwError::NotFound => f.write_str("Google could not find that spreadsheet or range"),
            GwError::Rejected(c) => write!(f, "Google rejected the request (HTTP {c})"),
            GwError::Network => f.write_str("could not reach Google"),
            GwError::BadResponse => f.write_str("Google's reply could not be read"),
            GwError::Invalid(m) => f.write_str(m),
        }
    }
}

fn invalid(m: impl Into<String>) -> GwError {
    GwError::Invalid(m.into())
}

fn map_http(e: crate::oidc::AuthError) -> GwError {
    use crate::oidc::AuthError;
    match e {
        AuthError::Unauthorized => GwError::Expired,
        AuthError::Rejected(403) => GwError::Forbidden,
        AuthError::Rejected(404) => GwError::NotFound,
        AuthError::Rejected(c) => GwError::Rejected(c),
        _ => GwError::Network,
    }
}

// ---------------------------------------------------------------------------
// Validation and request building (pure)
// ---------------------------------------------------------------------------

/// A spreadsheet id is the long token in its URL: letters, digits, `-` and `_`.
pub fn validate_spreadsheet_id(id: &str) -> Result<&str, GwError> {
    let id = id.trim();
    let ok = (20..=128).contains(&id.len())
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if ok {
        Ok(id)
    } else {
        Err(invalid(
            "that is not a Google spreadsheet id (the long code in the sheet's link)",
        ))
    }
}

/// An A1 range such as `Sheet1!A1:D20` or `'Q3 budget'!A:C`.
pub fn validate_range(range: &str) -> Result<&str, GwError> {
    let r = range.trim();
    if r.is_empty() || r.chars().count() > 200 || r.chars().any(char::is_control) {
        return Err(invalid(
            "the range must be an A1 range such as Sheet1!A1:D20",
        ));
    }
    Ok(r)
}

/// Percent-encode one URL path segment (RFC 3986 unreserved characters pass through).
fn pct_segment(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

pub fn values_url(id: &str, range: &str) -> Result<String, GwError> {
    let id = validate_spreadsheet_id(id)?;
    let range = validate_range(range)?;
    Ok(format!(
        "{SHEETS_API}/{id}/values/{}?majorDimension=ROWS&valueRenderOption=FORMATTED_VALUE",
        pct_segment(range)
    ))
}

pub fn append_url(id: &str, range: &str) -> Result<String, GwError> {
    let id = validate_spreadsheet_id(id)?;
    let range = validate_range(range)?;
    Ok(format!(
        "{SHEETS_API}/{id}/values/{}:append?valueInputOption=RAW&insertDataOption=INSERT_ROWS",
        pct_segment(range)
    ))
}

/// The append body. Only strings, finite numbers, booleans and nulls are accepted.
pub fn append_body(rows: &[Vec<Value>]) -> Result<Value, GwError> {
    if rows.is_empty() {
        return Err(invalid("there are no rows to add"));
    }
    if rows.len() > MAX_APPEND_ROWS {
        return Err(invalid(format!(
            "at most {MAX_APPEND_ROWS} rows can be added at once"
        )));
    }
    for (i, row) in rows.iter().enumerate() {
        if row.len() > MAX_APPEND_COLS {
            return Err(invalid(format!(
                "row {} has more than {MAX_APPEND_COLS} columns",
                i + 1
            )));
        }
        for cell in row {
            let ok = match cell {
                Value::Null | Value::Bool(_) => true,
                Value::Number(n) => n.as_f64().is_some_and(f64::is_finite),
                Value::String(s) => s.chars().count() <= MAX_CELL_CHARS,
                _ => false,
            };
            if !ok {
                return Err(invalid(format!(
                    "row {} has a cell that is not text, a number, true/false or empty (or is too long)",
                    i + 1
                )));
            }
        }
    }
    Ok(json!({ "majorDimension": "ROWS", "values": rows }))
}

/// The rows of a values reply (missing `values` = an empty range), at most [`MAX_READ_ROWS`].
pub fn parse_values(body: &str) -> Result<SheetValues, GwError> {
    let v: Value = serde_json::from_str(body).map_err(|_| GwError::BadResponse)?;
    let range = v
        .get("range")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let rows = match v.get("values") {
        None => Vec::new(),
        Some(Value::Array(rows)) => rows
            .iter()
            .map(|r| r.as_array().cloned().unwrap_or_default())
            .collect(),
        Some(_) => return Err(GwError::BadResponse),
    };
    let truncated = rows.len() > MAX_READ_ROWS;
    let rows = rows.into_iter().take(MAX_READ_ROWS).collect();
    Ok(SheetValues {
        range,
        rows,
        truncated,
    })
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SheetValues {
    pub range: String,
    pub rows: Vec<Vec<Value>>,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppendResult {
    pub updated_range: String,
    pub updated_rows: u64,
    pub updated_cells: u64,
}

pub fn parse_append(body: &str) -> Result<AppendResult, GwError> {
    let v: Value = serde_json::from_str(body).map_err(|_| GwError::BadResponse)?;
    let u = v.get("updates").ok_or(GwError::BadResponse)?;
    Ok(AppendResult {
        updated_range: u
            .get("updatedRange")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        updated_rows: u.get("updatedRows").and_then(Value::as_u64).unwrap_or(0),
        updated_cells: u.get("updatedCells").and_then(Value::as_u64).unwrap_or(0),
    })
}

// --- time ---------------------------------------------------------------------------------

/// Days since 1970-01-01 to (year, month, day), proleptic Gregorian (H. Hinnant's algorithm).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let m = i64::from(m);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Unix seconds as RFC 3339 UTC (`2026-10-01T09:30:00Z`).
pub fn rfc3339(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

fn digits(s: &str) -> Option<u32> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

/// Parse `YYYY-MM-DD` (all-day, midnight UTC) or RFC 3339 date-time with `Z` or `±HH:MM`.
pub fn parse_time(s: &str) -> Option<u64> {
    let s = s.trim();
    let (date, rest) = match s.split_once('T') {
        Some((d, r)) => (d, Some(r)),
        None => (s, None),
    };
    let mut parts = date.splitn(3, '-');
    let y = digits(parts.next()?)?;
    let mo = digits(parts.next()?)?;
    let d = digits(parts.next()?)?;
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) {
        return None;
    }
    let mut secs = days_from_civil(i64::from(y), mo, d) * 86_400;
    if let Some(rest) = rest {
        let (clock, offset) = if let Some(c) = rest.strip_suffix('Z') {
            (c, 0i64)
        } else {
            let i = rest.rfind(['+', '-'])?;
            let (c, o) = rest.split_at(i);
            let (sign, o) = match o.strip_prefix('-') {
                Some(r) => (-1, r),
                None => (1, o.strip_prefix('+')?),
            };
            let (oh, om) = o.split_once(':')?;
            (
                c,
                sign * (i64::from(digits(oh)?) * 3600 + i64::from(digits(om)?) * 60),
            )
        };
        let clock = clock.split('.').next()?;
        let mut hms = clock.splitn(3, ':');
        let h = digits(hms.next()?)?;
        let mi = digits(hms.next()?)?;
        let se = hms.next().map(digits).unwrap_or(Some(0))?;
        if h > 23 || mi > 59 || se > 60 {
            return None;
        }
        secs += i64::from(h) * 3600 + i64::from(mi) * 60 + i64::from(se) - offset;
    }
    u64::try_from(secs).ok()
}

// --- calendar -----------------------------------------------------------------------------

pub fn check_window(from: u64, to: u64) -> Result<(), GwError> {
    if to <= from {
        return Err(invalid("the calendar window must end after it starts"));
    }
    if to - from > MAX_WINDOW_DAYS * 86_400 {
        return Err(invalid(format!(
            "the calendar window is wider than {MAX_WINDOW_DAYS} days"
        )));
    }
    Ok(())
}

pub fn events_list_url(from: u64, to: u64) -> Result<String, GwError> {
    check_window(from, to)?;
    Ok(format!(
        "{CALENDAR_EVENTS_API}?singleEvents=true&orderBy=startTime&maxResults={MAX_EVENTS}&timeMin={}&timeMax={}",
        pct_segment(&rfc3339(from)),
        pct_segment(&rfc3339(to))
    ))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CalendarEvent {
    pub id: String,
    pub title: String,
    pub start: u64,
    pub end: u64,
    pub all_day: bool,
    /// Where the event is (free text from the calendar; shown as text, never opened).
    pub location: String,
}

fn event_time(v: &Value) -> Option<(u64, bool)> {
    if let Some(dt) = v.get("dateTime").and_then(Value::as_str) {
        return parse_time(dt).map(|t| (t, false));
    }
    v.get("date")
        .and_then(Value::as_str)
        .and_then(parse_time)
        .map(|t| (t, true))
}

/// The events of a listing reply. Cancelled events and events with unreadable times are skipped.
pub fn parse_events(body: &str) -> Result<Vec<CalendarEvent>, GwError> {
    let v: Value = serde_json::from_str(body).map_err(|_| GwError::BadResponse)?;
    let items = match v.get("items") {
        None => return Ok(Vec::new()),
        Some(Value::Array(a)) => a,
        Some(_) => return Err(GwError::BadResponse),
    };
    let text = |e: &Value, k: &str| {
        e.get(k)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .chars()
            .filter(|c| !c.is_control())
            .take(MAX_TITLE_CHARS)
            .collect::<String>()
    };
    Ok(items
        .iter()
        .filter(|e| e.get("status").and_then(Value::as_str) != Some("cancelled"))
        .filter_map(|e| {
            let (start, all_day) = event_time(e.get("start")?)?;
            let (end, _) = event_time(e.get("end")?)?;
            Some(CalendarEvent {
                id: text(e, "id"),
                title: {
                    let t = text(e, "summary");
                    if t.is_empty() {
                        "(no title)".to_string()
                    } else {
                        t
                    }
                },
                start,
                end,
                all_day,
                location: text(e, "location"),
            })
        })
        .take(MAX_EVENTS)
        .collect())
}

/// The body for creating a timed event.
pub fn event_body(title: &str, start: u64, end: u64, description: &str) -> Result<Value, GwError> {
    let title = title.trim();
    if title.is_empty()
        || title.chars().count() > MAX_TITLE_CHARS
        || title.chars().any(char::is_control)
    {
        return Err(invalid(format!(
            "the title must be 1 to {MAX_TITLE_CHARS} characters with no control characters"
        )));
    }
    if description.chars().count() > MAX_DESCRIPTION_CHARS {
        return Err(invalid(format!(
            "the description is longer than {MAX_DESCRIPTION_CHARS} characters"
        )));
    }
    if end <= start || end - start > 14 * 86_400 {
        return Err(invalid(
            "the event must end after it starts, within 14 days",
        ));
    }
    Ok(json!({
        "summary": title,
        "description": description,
        "start": { "dateTime": rfc3339(start) },
        "end": { "dateTime": rfc3339(end) },
    }))
}

// ---------------------------------------------------------------------------
// The client (HTTP seam + token source)
// ---------------------------------------------------------------------------

/// Calls Google with a sealed token. `token` is read per call so an expired or removed token is
/// never reused.
pub struct GoogleClient<'a> {
    pub http: &'a dyn HttpClient,
    pub token: &'a dyn Fn(Service) -> Option<zeroize::Zeroizing<String>>,
}

impl GoogleClient<'_> {
    fn bearer(&self, s: Service) -> Result<zeroize::Zeroizing<String>, GwError> {
        (self.token)(s).ok_or(GwError::NotConnected)
    }

    pub fn sheets_read(&self, id: &str, range: &str) -> Result<SheetValues, GwError> {
        let url = values_url(id, range)?;
        let t = self.bearer(Service::GoogleSheets)?;
        let body = self.http.get(&url, Some(t.as_str())).map_err(map_http)?;
        parse_values(&body)
    }

    pub fn sheets_append(
        &self,
        id: &str,
        range: &str,
        rows: &[Vec<Value>],
    ) -> Result<AppendResult, GwError> {
        let url = append_url(id, range)?;
        let body = append_body(rows)?;
        let t = self.bearer(Service::GoogleSheets)?;
        let resp = self
            .http
            .post_json(&url, Some(t.as_str()), &body.to_string())
            .map_err(map_http)?;
        parse_append(&resp)
    }

    pub fn calendar_list(&self, from: u64, to: u64) -> Result<Vec<CalendarEvent>, GwError> {
        let url = events_list_url(from, to)?;
        let t = self.bearer(Service::GoogleCalendar)?;
        let body = self.http.get(&url, Some(t.as_str())).map_err(map_http)?;
        parse_events(&body)
    }

    pub fn calendar_create(
        &self,
        title: &str,
        start: u64,
        end: u64,
        description: &str,
    ) -> Result<CalendarEvent, GwError> {
        let body = event_body(title, start, end, description)?;
        let t = self.bearer(Service::GoogleCalendar)?;
        let resp = self
            .http
            .post_json(CALENDAR_EVENTS_API, Some(t.as_str()), &body.to_string())
            .map_err(map_http)?;
        let wrapped = json!({ "items": [serde_json::from_str::<Value>(&resp).map_err(|_| GwError::BadResponse)?] });
        parse_events(&wrapped.to_string())?
            .into_iter()
            .next()
            .ok_or(GwError::BadResponse)
    }
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

/// One Google service's state for the UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GoogleServiceStatus {
    pub service: &'static str,
    /// A Google OAuth client id is configured (without it Connect stays disabled).
    pub configured: bool,
    pub connected: bool,
    /// Why it cannot be used yet, in the member's words (`None` when ready).
    pub note: Option<String>,
}

pub fn service_status(service: Service, configured: bool, connected: bool) -> GoogleServiceStatus {
    let note = if !configured {
        Some(GwError::NotConfigured.to_string())
    } else if !connected {
        Some(GwError::NotConnected.to_string())
    } else {
        None
    };
    GoogleServiceStatus {
        service: service.id(),
        configured,
        connected,
        note,
    }
}

fn with_client<T>(
    app: &tauri::AppHandle,
    f: impl FnOnce(&GoogleClient<'_>) -> Result<T, GwError>,
) -> Result<T, String> {
    let custody = tauri::Manager::try_state::<crate::custody::CustodyState>(app)
        .ok_or_else(|| "internal: managed state unavailable".to_string())?;
    let token = |s: Service| -> Option<zeroize::Zeroizing<String>> {
        crate::connections::sealed_access_token(s, &custody.0)
    };
    let http = crate::oidc::UreqClient;
    let client = GoogleClient {
        http: &http,
        token: &token,
    };
    f(&client).map_err(|e| e.to_string())
}

/// **google_workspace_status** — whether Sheets and Calendar are configured and connected.
#[tauri::command]
pub async fn google_workspace_status(
    app: tauri::AppHandle,
) -> Result<Vec<GoogleServiceStatus>, String> {
    crate::blocking::off_main(move || {
        let conns = tauri::Manager::try_state::<crate::connections::ConnectionState>(&app)
            .ok_or_else(|| "internal: managed state unavailable".to_string())?;
        let custody = tauri::Manager::try_state::<crate::custody::CustodyState>(&app)
            .ok_or_else(|| "internal: managed state unavailable".to_string())?;
        Ok([Service::GoogleSheets, Service::GoogleCalendar]
            .into_iter()
            .map(|s| {
                service_status(
                    s,
                    conns.0.is_configured(s),
                    crate::connections::sealed_access_token(s, &custody.0).is_some(),
                )
            })
            .collect())
    })
    .await
}

/// **gsheets_read** — read a range of a Google spreadsheet.
#[tauri::command]
pub async fn gsheets_read(
    app: tauri::AppHandle,
    spreadsheet_id: String,
    range: String,
) -> Result<SheetValues, String> {
    crate::blocking::off_main(move || with_client(&app, |c| c.sheets_read(&spreadsheet_id, &range)))
        .await
}

/// **gsheets_append** — the member adds rows to a Google spreadsheet (values stored as typed).
#[tauri::command]
pub async fn gsheets_append(
    app: tauri::AppHandle,
    spreadsheet_id: String,
    range: String,
    rows: Vec<Vec<Value>>,
) -> Result<AppendResult, String> {
    crate::blocking::off_main(move || {
        with_client(&app, |c| c.sheets_append(&spreadsheet_id, &range, &rows))
    })
    .await
}

/// **gcal_list** — events of the member's primary Google calendar in `[from, to)` (unix seconds).
#[tauri::command]
pub async fn gcal_list(
    app: tauri::AppHandle,
    from: u64,
    to: u64,
) -> Result<Vec<CalendarEvent>, String> {
    crate::blocking::off_main(move || with_client(&app, |c| c.calendar_list(from, to))).await
}

/// **gcal_create** — the member adds a timed event to their primary Google calendar.
#[tauri::command]
pub async fn gcal_create(
    app: tauri::AppHandle,
    title: String,
    start: u64,
    end: u64,
    description: String,
) -> Result<CalendarEvent, String> {
    crate::blocking::off_main(move || {
        with_client(&app, |c| {
            c.calendar_create(&title, start, end, &description)
        })
    })
    .await
}

#[cfg(test)]
mod tests {
    include!("google_workspace_tests.rs");
}
