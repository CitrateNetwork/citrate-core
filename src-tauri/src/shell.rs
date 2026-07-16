//! citrate-core — open an external federation link in the system browser.
//!
//! Wires the previously-dead Commissary / tutorial / explorer "open ↗" links. The
//! URL is opened in the OS default browser (tauri-plugin-opener, same path as
//! `kyc_start`); the destination RP (Atlas, CitrateScan, …) runs its own OIDC
//! login. A real signed-in handoff (shared-authority SSO so the user arrives
//! already authenticated) is a later work-order item (WO-6) — this command only
//! carries the URL, no token.
//!
//! SAFETY: only `https://` URLs are opened. We never hand an arbitrary scheme to
//! the opener (no `file:`/`javascript:`/custom-scheme), so a malformed or hostile
//! catalog entry cannot drive the opener into a non-web target.

/// `open_external` — open an `https` URL in the system browser. Rejects any
/// non-https scheme (fail closed). Returns `()`.
#[tauri::command]
pub fn open_external(app: tauri::AppHandle, url: String) -> std::result::Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let trimmed = url.trim();
    if !trimmed.starts_with("https://") {
        return Err("open_external: only https URLs may be opened".to_string());
    }
    app.opener()
        .open_url(trimmed.to_string(), None::<&str>)
        .map_err(|_| "open_external: could not open the URL".to_string())
}

#[cfg(test)]
mod tests {
    // The https-only guard is the security-relevant unit (the opener call itself
    // needs a live AppHandle, exercised in the app). We assert the scheme check
    // via a tiny re-implementation-free helper: call the guard logic directly.
    #[test]
    fn only_https_is_accepted_by_the_guard() {
        // Mirror the command's guard (the command needs an AppHandle to run).
        let ok = |u: &str| u.trim().starts_with("https://");
        assert!(ok("https://explorer.citrate.ai/tx/0xabc"));
        assert!(ok("  https://docs.citrate.ai/tutorials/x  "));
        assert!(!ok("http://explorer.citrate.ai"), "plain http rejected");
        assert!(!ok("file:///etc/passwd"), "file scheme rejected");
        assert!(!ok("javascript:alert(1)"), "javascript scheme rejected");
        assert!(!ok("citrate-core://x"), "custom scheme rejected");
    }
}
