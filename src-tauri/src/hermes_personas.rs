//! HUP-S3.3 + S3.7 (core half): Hermes personas and track workflows.
//!
//! The runtime (`citrate-agent-runtime` agent-loop `personas` + `workflows`) owns the data: the
//! shipped personas live in one file there, with names that are placeholders pending owner
//! sign-off. Core reads them from the sidecar (`GET /personas`, `GET /workflows`) and sends a
//! member-defined persona to `POST /personas/check`, which validates it and renders its
//! system-prompt fragment with the same template as the shipped ones. Core bounds the input before
//! it leaves the app; the sidecar owns the rules.
//!
//! The chosen persona only changes the prompt's voice. It never grants a tool and never changes
//! approvals, gates or the SignatureCeremony. Nothing here signs, spends or writes.

use super::{manager, HermesError, HermesManager, Result as HResult};
use serde::{Deserialize, Serialize};

const MAX_NAME_CHARS: usize = 40;
const MAX_SUMMARY_CHARS: usize = 200;
const MAX_VOICE_CHARS: usize = 300;
const MAX_RULE_CHARS: usize = 300;
const MAX_RULES: usize = 12;
const MAX_TOOLS: usize = 12;
const MAX_SKILLS: usize = 24;

/// A persona as the sidecar serves it (agent-loop `PersonaView`, snake_case, flattened).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HermesPersona {
    pub id: String,
    pub role: String,
    pub name: String,
    pub name_status: String,
    pub summary: String,
    pub voice: String,
    pub tone: String,
    pub style_rules: Vec<String>,
    pub default_track: String,
    pub default_workflow: String,
    pub tool_emphasis: Vec<String>,
    pub skills: Vec<String>,
    #[serde(default)]
    pub tts_voice: Option<String>,
    pub prompt_fragment: String,
    pub name_pending_sign_off: bool,
    pub custom: bool,
}

/// A member-defined persona, as the settings form submits it (agent-loop `CustomPersona`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CustomPersonaInput {
    pub id: String,
    pub name: String,
    pub summary: String,
    pub voice: String,
    pub tone: String,
    pub style_rules: Vec<String>,
    pub default_track: String,
    #[serde(default)]
    pub tool_emphasis: Vec<String>,
    #[serde(default)]
    pub skills: Vec<String>,
    #[serde(default)]
    pub tts_voice: Option<String>,
}

/// One step of a track workflow (agent-loop `StepView`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TrackWorkflowStep {
    pub id: String,
    pub instruction: String,
    pub max_attempts: u32,
    pub verifier_names: Vec<String>,
}

/// One workflow of a track's family (agent-loop `WorkflowView`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TrackWorkflow {
    pub id: String,
    pub track: String,
    pub title: String,
    pub summary: String,
    pub is_default: bool,
    /// "tool-report" or "answer-shape".
    pub evidence: String,
    pub tools: Vec<String>,
    pub verifier_names: Vec<String>,
    pub steps: Vec<TrackWorkflowStep>,
}

fn slug(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

fn bounded(s: &str, max: usize, what: &str) -> std::result::Result<(), String> {
    if s.trim().is_empty() || s.chars().count() > max {
        Err(format!("{what} must be 1 to {max} characters"))
    } else {
        Ok(())
    }
}

/// Size and shape bounds; the sidecar applies the real rules (reserved names, known tracks).
pub fn validate_custom_input(p: &CustomPersonaInput) -> std::result::Result<(), String> {
    match p.id.strip_prefix("custom-") {
        Some(rest) if !rest.is_empty() && slug(&p.id) => {}
        _ => return Err("a custom persona id is custom-<lowercase slug>".into()),
    }
    bounded(&p.name, MAX_NAME_CHARS, "the name")?;
    bounded(&p.summary, MAX_SUMMARY_CHARS, "the summary")?;
    bounded(&p.voice, MAX_VOICE_CHARS, "the voice")?;
    bounded(&p.tone, MAX_VOICE_CHARS, "the tone")?;
    if p.style_rules.is_empty() || p.style_rules.len() > MAX_RULES {
        return Err(format!("1 to {MAX_RULES} style rules"));
    }
    for r in &p.style_rules {
        bounded(r, MAX_RULE_CHARS, "a style rule")?;
    }
    if !slug(&p.default_track) {
        return Err("the default track is a track id".into());
    }
    if p.tool_emphasis.len() > MAX_TOOLS
        || !p.tool_emphasis.iter().all(|t| {
            !t.is_empty()
                && t.len() <= 64
                && t.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        })
    {
        return Err(format!("at most {MAX_TOOLS} tool names"));
    }
    if p.skills.len() > MAX_SKILLS || !p.skills.iter().all(|s| slug(s)) {
        return Err(format!("at most {MAX_SKILLS} skill names"));
    }
    if let Some(v) = &p.tts_voice {
        let ok = !v.is_empty()
            && v.len() <= 64
            && v.bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'));
        if !ok {
            return Err("a TTS voice id is letters, digits, dot, dash or underscore".into());
        }
    }
    Ok(())
}

fn get<T: serde::de::DeserializeOwned>(m: &HermesManager, path: &str) -> HResult<T> {
    let bearer = m.bearer()?;
    let resp = m
        .control
        .get(&format!("{}{path}", m.control_url()), &bearer)?;
    HermesManager::decode(resp)
}

/// `GET /personas`: the shipped personas with their fragments.
pub fn personas(m: &HermesManager) -> std::result::Result<Vec<HermesPersona>, String> {
    get(m, "/personas").map_err(|e| e.to_string())
}

/// `GET /workflows`: every track's workflow family.
pub fn workflows(m: &HermesManager) -> std::result::Result<Vec<TrackWorkflow>, String> {
    get(m, "/workflows").map_err(|e| e.to_string())
}

/// `POST /personas/check`: the sidecar validates a custom persona and renders its fragment. A
/// refusal reads `PERSONA_REFUSED: <reason>`.
pub fn persona_check(
    m: &HermesManager,
    p: &CustomPersonaInput,
) -> std::result::Result<HermesPersona, String> {
    validate_custom_input(p).map_err(|e| format!("PERSONA_REFUSED: {e}"))?;
    let run = || -> HResult<HermesPersona> {
        let bearer = m.bearer()?;
        let body = serde_json::json!({ "persona": p }).to_string();
        let resp = m.control.post(
            &format!("{}/personas/check", m.control_url()),
            &bearer,
            &body,
        )?;
        if resp.status == 422 {
            let reason = serde_json::from_str::<serde_json::Value>(&resp.body)
                .ok()
                .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(str::to_string))
                .filter(|r| !r.trim().is_empty())
                .unwrap_or_else(|| "the sidecar refused this persona".to_string());
            return Err(HermesError::Control {
                status: 422,
                msg: reason.chars().take(300).collect(),
            });
        }
        HermesManager::decode(resp)
    };
    run().map_err(|e| match e {
        HermesError::Control { status: 422, msg } => format!("PERSONA_REFUSED: {msg}"),
        other => other.to_string(),
    })
}

/// **hermes_personas**: the shipped personas (names pending owner sign-off).
#[tauri::command]
pub async fn hermes_personas(
    app: tauri::AppHandle,
) -> std::result::Result<Vec<HermesPersona>, String> {
    crate::blocking::off_main(move || personas(manager(&app)?)).await
}

/// **hermes_workflows**: every track's workflow family (definitions; nothing runs).
#[tauri::command]
pub async fn hermes_workflows(
    app: tauri::AppHandle,
) -> std::result::Result<Vec<TrackWorkflow>, String> {
    crate::blocking::off_main(move || workflows(manager(&app)?)).await
}

/// **hermes_persona_check**: validate a member-defined persona and get its fragment.
#[tauri::command]
pub async fn hermes_persona_check(
    app: tauri::AppHandle,
    persona: CustomPersonaInput,
) -> std::result::Result<HermesPersona, String> {
    crate::blocking::off_main(move || persona_check(manager(&app)?, &persona)).await
}

#[cfg(test)]
#[path = "hermes_personas_tests.rs"]
mod tests;
