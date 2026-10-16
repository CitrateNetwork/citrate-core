// HUP-S10.2 (US-10.2 AC2) — Google Sheets + Calendar over Connections: request shapes, reply
// parsing, the "not configured / not connected / expired" states, and that a token only ever
// travels to Google.

use super::*;
use std::sync::Mutex;

const SHEET: &str = "1AbCdEfGhIjKlMnOpQrStUvWxYz0123456789_-xy";

/// (method, url, bearer, body)
type Call = (String, String, Option<String>, String);

#[derive(Default)]
struct Fake {
    calls: Mutex<Vec<Call>>,
    reply: Mutex<Vec<std::result::Result<String, crate::oidc::AuthError>>>,
}

impl crate::oidc::HttpClient for Fake {
    fn get(
        &self,
        url: &str,
        bearer: Option<&str>,
    ) -> std::result::Result<String, crate::oidc::AuthError> {
        self.calls.lock().unwrap().push((
            "GET".into(),
            url.into(),
            bearer.map(str::to_string),
            String::new(),
        ));
        self.reply.lock().unwrap().remove(0)
    }
    fn post_form(
        &self,
        _url: &str,
        _form: &[(&str, &str)],
    ) -> std::result::Result<String, crate::oidc::AuthError> {
        panic!("the Google client never posts a form")
    }
    fn post_json(
        &self,
        url: &str,
        bearer: Option<&str>,
        body: &str,
    ) -> std::result::Result<String, crate::oidc::AuthError> {
        self.calls.lock().unwrap().push((
            "POST".into(),
            url.into(),
            bearer.map(str::to_string),
            body.into(),
        ));
        self.reply.lock().unwrap().remove(0)
    }
}

fn fake(replies: Vec<std::result::Result<String, crate::oidc::AuthError>>) -> Fake {
    Fake {
        calls: Mutex::new(vec![]),
        reply: Mutex::new(replies),
    }
}

fn connected(s: Service) -> Option<zeroize::Zeroizing<String>> {
    Some(zeroize::Zeroizing::new(format!("tok-{}", s.id())))
}

fn nobody(_s: Service) -> Option<zeroize::Zeroizing<String>> {
    None
}

#[test]
fn the_two_services_are_google_with_the_narrowest_scopes() {
    assert_eq!(Service::from_id("gsheets"), Some(Service::GoogleSheets));
    assert_eq!(Service::from_id("gcal"), Some(Service::GoogleCalendar));
    assert_eq!(
        Service::GoogleSheets.default_scopes(),
        &["https://www.googleapis.com/auth/spreadsheets"]
    );
    assert_eq!(
        Service::GoogleCalendar.default_scopes(),
        &["https://www.googleapis.com/auth/calendar.events"]
    );
    for s in [Service::GoogleSheets, Service::GoogleCalendar] {
        assert_eq!(s.token_endpoint(), "https://oauth2.googleapis.com/token");
        let url = crate::connections::authorize_url(s, "cid", "st", "ch");
        assert!(url.starts_with("https://accounts.google.com/o/oauth2/v2/auth?"));
        assert!(url.contains("access_type=offline"), "{url}");
        assert!(url.contains("code_challenge_method=S256"), "{url}");
    }
}

#[test]
fn status_is_honest_about_configuration_and_connection() {
    let s = service_status(Service::GoogleSheets, false, false);
    assert!(!s.configured);
    assert!(s.note.as_deref().unwrap().contains("client id"));
    let s = service_status(Service::GoogleCalendar, true, false);
    assert!(s.note.as_deref().unwrap().contains("not connected"));
    let s = service_status(Service::GoogleCalendar, true, true);
    assert_eq!(s.note, None);
    assert_eq!(s.service, "gcal");
}

#[test]
fn nothing_goes_out_when_not_connected() {
    let http = fake(vec![]);
    let c = GoogleClient {
        http: &http,
        token: &nobody,
    };
    assert_eq!(c.sheets_read(SHEET, "A1:B2"), Err(GwError::NotConnected));
    assert_eq!(
        c.calendar_list(1_790_812_800, 1_790_812_800 + 86_400),
        Err(GwError::NotConnected)
    );
    assert!(http.calls.lock().unwrap().is_empty());
}

#[test]
fn a_sheet_read_sends_the_sheets_token_to_google_only_and_parses_rows() {
    let http = fake(vec![Ok(
        r#"{"range":"'Q3 budget'!A1:B3","majorDimension":"ROWS","values":[["item","cost"],["paper","4.50"],["pens"]]}"#
            .into(),
    )]);
    let c = GoogleClient {
        http: &http,
        token: &connected,
    };
    let v = c.sheets_read(SHEET, "'Q3 budget'!A1:B3").unwrap();
    assert_eq!(v.rows.len(), 3);
    assert_eq!(v.rows[2], vec![json!("pens")]);
    assert!(!v.truncated);
    let calls = http.calls.lock().unwrap();
    let (method, url, bearer, _) = &calls[0];
    assert_eq!(method, "GET");
    assert!(
        url.starts_with(&format!("{SHEETS_API}/{SHEET}/values/")),
        "{url}"
    );
    // The range is one encoded path segment: no raw quote, space, ! or / reaches the URL path.
    assert!(url.contains("%27Q3%20budget%27%21A1%3AB3"), "{url}");
    assert_eq!(bearer.as_deref(), Some("tok-gsheets"));
}

#[test]
fn bad_ids_and_ranges_are_refused_before_any_request() {
    let http = fake(vec![]);
    let c = GoogleClient {
        http: &http,
        token: &connected,
    };
    for id in [
        "short",
        "../../evil-path-segment-here",
        "a b c d e f g h i j k l m n o p",
    ] {
        assert!(
            matches!(c.sheets_read(id, "A1"), Err(GwError::Invalid(_))),
            "{id}"
        );
    }
    assert!(matches!(c.sheets_read(SHEET, ""), Err(GwError::Invalid(_))));
    assert!(matches!(
        c.sheets_read(SHEET, "A1\nB2"),
        Err(GwError::Invalid(_))
    ));
    assert!(http.calls.lock().unwrap().is_empty());
}

#[test]
fn an_append_is_raw_so_google_never_runs_a_formula() {
    let http = fake(vec![Ok(
        r#"{"updates":{"updatedRange":"Sheet1!A4:B4","updatedRows":1,"updatedCells":2}}"#.into(),
    )]);
    let c = GoogleClient {
        http: &http,
        token: &connected,
    };
    let r = c
        .sheets_append(
            SHEET,
            "Sheet1!A:B",
            &[vec![json!("=IMPORTXML(\"x\")"), json!(3)]],
        )
        .unwrap();
    assert_eq!(r.updated_cells, 2);
    let calls = http.calls.lock().unwrap();
    let (method, url, _, body) = &calls[0];
    assert_eq!(method, "POST");
    assert!(url.contains(":append?valueInputOption=RAW"), "{url}");
    let b: Value = serde_json::from_str(body).unwrap();
    assert_eq!(b["values"][0][0], "=IMPORTXML(\"x\")");
}

#[test]
fn append_limits_and_cell_types_are_enforced() {
    assert!(append_body(&[]).is_err());
    let many: Vec<Vec<Value>> = (0..=MAX_APPEND_ROWS).map(|_| vec![json!(1)]).collect();
    assert!(append_body(&many).is_err());
    assert!(append_body(&[vec![json!({"a": 1})]]).is_err());
    assert!(append_body(&[vec![json!([1])]]).is_err());
    assert!(append_body(&[vec![json!("x".repeat(MAX_CELL_CHARS + 1))]]).is_err());
    assert!(append_body(&[vec![json!("ok"), json!(1.5), json!(true), Value::Null]]).is_ok());
}

#[test]
fn google_status_codes_become_member_facing_states() {
    use crate::oidc::AuthError;
    let http = fake(vec![
        Err(AuthError::Unauthorized),
        Err(AuthError::Rejected(403)),
        Err(AuthError::Rejected(404)),
        Err(AuthError::Rejected(429)),
        Err(AuthError::Network),
        Ok("not json".into()),
    ]);
    let c = GoogleClient {
        http: &http,
        token: &connected,
    };
    let r = || c.sheets_read(SHEET, "A1");
    assert_eq!(r(), Err(GwError::Expired));
    assert_eq!(r(), Err(GwError::Forbidden));
    assert_eq!(r(), Err(GwError::NotFound));
    assert_eq!(r(), Err(GwError::Rejected(429)));
    assert_eq!(r(), Err(GwError::Network));
    assert_eq!(r(), Err(GwError::BadResponse));
    assert!(GwError::Expired.to_string().contains("reconnect"));
}

#[test]
fn rfc3339_and_parse_time_agree() {
    assert_eq!(rfc3339(0), "1970-01-01T00:00:00Z");
    assert_eq!(rfc3339(1_790_812_800), "2026-10-01T00:00:00Z");
    assert_eq!(rfc3339(951_782_400 + 3_723), "2000-02-29T01:02:03Z");
    for t in [0u64, 951_782_400, 1_790_812_800 + 45_296, 4_102_444_800] {
        assert_eq!(parse_time(&rfc3339(t)), Some(t), "{t}");
    }
    assert_eq!(parse_time("2026-10-01"), Some(1_790_812_800));
    assert_eq!(
        parse_time("2026-10-01T09:00:00-07:00"),
        Some(1_790_812_800 + 16 * 3600)
    );
    assert_eq!(
        parse_time("2026-10-01T09:00:00.250+02:00"),
        Some(1_790_812_800 + 7 * 3600)
    );
    for bad in [
        "",
        "2026-13-01",
        "2026-10-01T25:00:00Z",
        "2026-10-01T09:00",
        "x-y-z",
    ] {
        assert_eq!(parse_time(bad), None, "{bad}");
    }
}

#[test]
fn calendar_listing_asks_for_single_events_in_the_window_and_skips_cancelled() {
    let from = 1_790_812_800;
    let to = from + 7 * 86_400;
    let http = fake(vec![Ok(json!({
        "items": [
            {"id": "a", "summary": "Standup", "status": "confirmed",
             "start": {"dateTime": "2026-10-01T09:00:00Z"}, "end": {"dateTime": "2026-10-01T09:15:00Z"}},
            {"id": "b", "summary": "Gone", "status": "cancelled",
             "start": {"dateTime": "2026-10-02T09:00:00Z"}, "end": {"dateTime": "2026-10-02T10:00:00Z"}},
            {"id": "c", "start": {"date": "2026-10-03"}, "end": {"date": "2026-10-04"}, "location": "Home\u{7}"},
            {"id": "d", "summary": "no times"}
        ]
    })
    .to_string())]);
    let c = GoogleClient {
        http: &http,
        token: &connected,
    };
    let ev = c.calendar_list(from, to).unwrap();
    assert_eq!(ev.len(), 2);
    assert_eq!(ev[0].title, "Standup");
    assert_eq!(ev[0].start, from + 9 * 3600);
    assert!(!ev[0].all_day);
    assert_eq!(ev[1].title, "(no title)");
    assert!(ev[1].all_day);
    assert_eq!(ev[1].location, "Home");
    let calls = http.calls.lock().unwrap();
    let (_, url, bearer, _) = &calls[0];
    assert!(url.starts_with(CALENDAR_EVENTS_API), "{url}");
    assert!(url.contains("singleEvents=true"), "{url}");
    assert!(url.contains("timeMin=2026-10-01T00%3A00%3A00Z"), "{url}");
    assert!(url.contains("timeMax=2026-10-08T00%3A00%3A00Z"), "{url}");
    assert_eq!(bearer.as_deref(), Some("tok-gcal"));
}

#[test]
fn calendar_windows_are_bounded() {
    let http = fake(vec![]);
    let c = GoogleClient {
        http: &http,
        token: &connected,
    };
    assert!(matches!(c.calendar_list(10, 5), Err(GwError::Invalid(_))));
    assert!(matches!(
        c.calendar_list(0, (MAX_WINDOW_DAYS + 1) * 86_400),
        Err(GwError::Invalid(_))
    ));
    assert!(http.calls.lock().unwrap().is_empty());
}

#[test]
fn creating_an_event_posts_a_timed_event_and_reads_it_back() {
    let start = 1_790_812_800 + 10 * 3600;
    let http = fake(vec![Ok(json!({
        "id": "new1", "summary": "Review", "status": "confirmed",
        "start": {"dateTime": rfc3339(start)}, "end": {"dateTime": rfc3339(start + 1800)}
    })
    .to_string())]);
    let c = GoogleClient {
        http: &http,
        token: &connected,
    };
    let e = c
        .calendar_create("Review", start, start + 1800, "weekly")
        .unwrap();
    assert_eq!(e.id, "new1");
    let calls = http.calls.lock().unwrap();
    let (_, url, _, body) = &calls[0];
    assert_eq!(url, CALENDAR_EVENTS_API);
    let b: Value = serde_json::from_str(body).unwrap();
    assert_eq!(b["start"]["dateTime"], "2026-10-01T10:00:00Z");
    assert_eq!(b["summary"], "Review");
}

#[test]
fn bad_events_are_refused() {
    assert!(event_body("", 10, 20, "").is_err());
    assert!(event_body("x", 20, 10, "").is_err());
    assert!(event_body("x", 0, 15 * 86_400, "").is_err());
    assert!(event_body("a\u{1b}b", 0, 60, "").is_err());
    assert!(event_body("x", 0, 60, &"d".repeat(MAX_DESCRIPTION_CHARS + 1)).is_err());
}

#[test]
fn values_reads_are_capped() {
    let rows: Vec<Value> = (0..(MAX_READ_ROWS + 5)).map(|i| json!([i])).collect();
    let v = parse_values(&json!({"range": "A1", "values": rows}).to_string()).unwrap();
    assert_eq!(v.rows.len(), MAX_READ_ROWS);
    assert!(v.truncated);
    let v = parse_values(r#"{"range":"A1"}"#).unwrap();
    assert!(v.rows.is_empty());
}
