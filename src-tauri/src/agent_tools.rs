//! CX-S6.4 (lane s6) — the Hermes code-task routing surface, under the HIC model.
//!
//! HIC (Human In Control) is the federation-wide, normative standard (citrate-quorum
//! `04_HIC_MODEL.md`): HIC-0 observes, **HIC-1 pauses every action for the human**, HIC-2 is
//! budgeted autonomy under a grant, HIC-X is ungoverned/quarantined. A Hermes agent is keyless and
//! runs each effect at **HIC-1** — the human decides every one. The two effect shapes differ only in
//! the MECHANISM of that HIC-1 decision:
//!
//! - a **chain effect** (carries `to` + `data`) → the [`SignatureCeremony`](crate::ceremony) (S6.3):
//!   the HIC-1 decision is a real signature the human makes; the agent holds no key.
//! - a **code / shell task** (a WASM-capsule skill or an `agent-code` tool: file ops, shell_exec) →
//!   a HIC-1 **control decision** (`/approvals/approve|reject`): the human lets it proceed or aborts
//!   it. No signature is involved, and there is NO auto-run for code — the only budgeted-autonomy
//!   (HIC-2) relaxation in this design is chain-only (SessionBudget). Every code/shell task stops for
//!   an explicit human decision. The capsule sandbox (wasmtime 64 MiB + the armed epoch deadline) is
//!   the isolation; HIC-1 is the authorization.
//!
//! This module is the single place that classifies an approval into the frozen `AgentHarnessDomain`
//! kind (`"chain" | "code" | "shell"`) and its HIC-1 resolution route, so the surface + UI agree.
//!
//! NOTE FOR FUTURE DEVELOPERS — terminology. **HIC-1 is this federation's standard for what the
//! wider industry (and some upstream/vendored code, e.g. agent-core's `hitl` module) calls
//! "human-in-the-loop" (HITL).** We use **HIC (Human In Control)** deliberately: it centers the
//! human's authority over the action, not a "loop" the human is a cog in — and it avoids a term we
//! find both belittling and, written down, uncomfortably close to another word. When you see "HITL"
//! anywhere, read it as HIC-1 and prefer HIC in new code, docs, and UI. The graded model
//! (HIC-0 observe · HIC-1 every action stops for the human · HIC-2 budgeted autonomy under a grant ·
//! HIC-X ungoverned/quarantined) is normative federation-wide — see citrate-quorum `04_HIC_MODEL.md`.

use super::PendingApproval;

/// The `AgentHarnessDomain` kind for an approval — and, by that kind, its HIC-1 route:
///
/// - `"chain"` (carries `to`+`data`) → the human's HIC-1 decision is a **signature** in the
///   SignatureCeremony (S6.3).
/// - `"shell"` / `"code"` → the human's HIC-1 decision is an explicit **approve/reject** (no
///   signature); a task naming a shell tool is surfaced as `shell` so the UI can warn louder
///   (shell_exec is the highest-risk `agent-code` tool). There is no auto-run for code — the only
///   budgeted-autonomy (HIC-2) relaxation in this design is chain-only (SessionBudget).
///
/// The UI reads this single string to pick the right HIC-1 control; the route is derivable from it,
/// so it is the one source of truth (no parallel Rust enum to drift).
pub fn classify_kind(a: &PendingApproval) -> &'static str {
    if a.to.is_some() && a.data.is_some() {
        return "chain";
    }
    let name = a.id.to_ascii_lowercase();
    if name.contains("shell") || name.contains("exec") || name.contains("bash") {
        "shell"
    } else {
        "code"
    }
}

/// Normalize a batch of approvals to the domain kind vocabulary (`chain|code|shell`) in place — the
/// sidecar surfaces a coarse risk level; the UI needs the domain kind to pick the HIC-1 control.
pub fn normalize_kinds(approvals: &mut [PendingApproval]) {
    for a in approvals.iter_mut() {
        a.kind = classify_kind(a).to_string();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approval(id: &str, to: Option<&str>, data: Option<&str>) -> PendingApproval {
        PendingApproval {
            id: id.to_string(),
            kind: "high".to_string(), // the coarse sidecar risk level, to be normalized
            summary: String::new(),
            to: to.map(str::to_string),
            data: data.map(str::to_string),
        }
    }

    #[test]
    fn a_chain_effect_is_classified_chain() {
        let a = approval("cap::eth-send", Some("0xabc"), Some("0x01"));
        assert_eq!(
            classify_kind(&a),
            "chain",
            "signed in the ceremony (HIC-1 by signature)"
        );
    }

    #[test]
    fn a_shell_task_is_flagged_shell() {
        let a = approval("shell_exec", None, None);
        assert_eq!(
            classify_kind(&a),
            "shell",
            "HIC-1 control decision; UI warns louder"
        );
    }

    #[test]
    fn a_plain_code_task_is_classified_code() {
        let a = approval("file_write", None, None);
        assert_eq!(
            classify_kind(&a),
            "code",
            "HIC-1 control decision (approve/reject)"
        );
    }

    #[test]
    fn normalize_rewrites_the_coarse_risk_level_to_the_domain_kind() {
        let mut batch = vec![
            approval("cap::eth-send", Some("0xabc"), Some("0x01")),
            approval("shell_exec", None, None),
            approval("summarize", None, None),
        ];
        normalize_kinds(&mut batch);
        assert_eq!(batch[0].kind, "chain");
        assert_eq!(batch[1].kind, "shell");
        assert_eq!(batch[2].kind, "code");
    }
}
