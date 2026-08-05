//! Link this device's custody EOA to the member's Citrate identity — through the
//! signature ceremony, like every other use of the wallet key.
//!
//! # Why this exists
//!
//! The authority mints a `wallet_address` claim for every member. Until a wallet
//! is explicitly bound it is the *counterfactual* CREATE2 smart-wallet address —
//! an address no private key can spend from. core-membership records that claim
//! as the member's pay-to address, and the treasury bond-funds it with the 32,000
//! SALT the member needs to self-bond as a validator. But the self-bond is sent
//! from THIS app's custody EOA (`wallet::address`), which the authority has never
//! heard of. Funding the predicted address would strand the bond somewhere the
//! member cannot reach.
//!
//! Linking closes that: the authority's identity↔wallet registry accepts a wallet
//! that proves control of itself, and a proven *canonical* link becomes the
//! member's bound `primaryWallet` — which the `wallet_address` claim then serves.
//!
//! # Why a ceremony
//!
//! The proof is an EIP-191 signature by the custody key. Every signature this app
//! produces goes through [`crate::ceremony`] with explicit human approval (Rule 3
//! / HIC), and this one is no exception — the human sees the exact message
//! (`decode_personal_sign` renders it verbatim) and approves it by id. Nothing
//! here can sign on its own: this module only *requests* a ceremony and, after
//! the human approves, forwards the resulting signature to the authority.
//!
//! No funds move. The message binds authority, identity, wallet, chain id and a
//! one-time nonce, so a proof cannot be replayed or retargeted at another
//! identity.
//!
//! # The two-step shape
//!
//! A challenge nonce is issued *before* the human approves and must be echoed on
//! submit, so the nonce and the address are held against the ceremony id between
//! request and approval. The nonce is one-time: if submission fails the whole
//! flow restarts with a fresh challenge rather than reusing a spent one.

use std::collections::HashMap;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::ceremony::{CeremonyView, IntentKind, SignatureIntent, SignatureCeremony};
use crate::custody::CustodyVault;
use crate::oidc::wallet_link_message;

/// What the caller learns after a successful link. Deliberately NOT the
/// signature: a proof that has already been spent has no further use to the UI,
/// and echoing signatures across the bridge is how they end up in logs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WalletLinkResult {
    /// The address now bound to the identity (EIP-55 as the wallet reports it).
    pub address: String,
    /// True once the authority has accepted the proof.
    pub linked: bool,
    /// True when the authority now serves THIS address as `wallet_address`.
    ///
    /// Separate from `linked` because they can genuinely differ: the link is
    /// durable the moment the proof is accepted, but the CLAIM only moves if the
    /// address is also canonical. Reporting one bit for both would let the UI
    /// declare success while the member stays blocked — the exact failure this
    /// field exists to make visible.
    pub canonical: bool,
}

/// Errors surfaced to the UI. Coarse and secret-free — never the bearer, never
/// the key, never the signature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkError {
    /// No signed-in session (or no `sub`) to link against.
    NotSignedIn,
    /// The vault is locked or holds no wallet.
    Wallet(String),
    /// The authority could not be reached, or refused the challenge/submit.
    Authority(String),
    /// The challenge message did not carry the expected placeholder — the
    /// authority's format changed and we refuse to sign what we cannot build.
    UnexpectedChallenge,
    /// No pending link for this ceremony id (already used, or never requested).
    UnknownLink,
    /// The ceremony refused (locked vault, unknown id, raw-ack required).
    Ceremony(String),
}

impl std::fmt::Display for LinkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LinkError::NotSignedIn => write!(f, "sign in before linking a wallet"),
            LinkError::Wallet(e) => write!(f, "wallet unavailable: {e}"),
            LinkError::Authority(e) => write!(f, "authority rejected the link: {e}"),
            LinkError::UnexpectedChallenge => write!(
                f,
                "the authority's link challenge had an unexpected format — refusing to sign it"
            ),
            LinkError::UnknownLink => write!(f, "no pending wallet link for that ceremony"),
            LinkError::Ceremony(e) => write!(f, "{e}"),
        }
    }
}

/// The proof to submit once the human has approved. In-process only — the
/// signature must never cross the invoke bridge (I-2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkProof {
    pub address: String,
    /// `0x`-prefixed EIP-191 signature (`r||s||v`).
    pub signature: String,
    pub nonce: String,
}

/// One in-flight link, held against its ceremony id.
#[derive(Debug, Clone)]
struct PendingLink {
    nonce: String,
    address: String,
}

/// Process-wide pending-link table. Small and bounded by the number of ceremonies
/// a human can have open at once.
#[derive(Default)]
pub struct WalletLinkState {
    pending: Mutex<HashMap<String, PendingLink>>,
}

impl WalletLinkState {
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, PendingLink>> {
        self.pending.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Step 1 — build the exact message from an authority challenge and open a
    /// ceremony for it.
    ///
    /// The HTTP calls live OUTSIDE this crate (see `AuthManager::wallet_link_*`):
    /// this module is pure orchestration over injected inputs, so the whole flow —
    /// message construction, ceremony gating, pending bookkeeping — is testable
    /// without a live authority or a network.
    ///
    /// The caller must read the wallet address BEFORE requesting a challenge, so a
    /// locked vault does not burn a one-time nonce.
    pub fn open(
        &self,
        ceremony: &SignatureCeremony,
        chain_id: u64,
        origin: &str,
        address: &str,
        nonce: &str,
        message_template: &str,
    ) -> Result<CeremonyView, LinkError> {
        // Substitute, never re-derive: the authority verifies against the string
        // IT rebuilds, so a locally-formatted message would produce a proof that
        // silently fails to recover.
        let message =
            wallet_link_message(message_template, address).ok_or(LinkError::UnexpectedChallenge)?;

        let view = ceremony.request(SignatureIntent {
            // The authority is the true asker; shown verbatim in the approval UI.
            origin: origin.to_string(),
            kind: IntentKind::PersonalSign,
            chain_id,
            raw: hex::encode(message.as_bytes()),
        });

        self.lock().insert(
            view.id.clone(),
            PendingLink {
                nonce: nonce.to_string(),
                address: address.to_string(),
            },
        );
        Ok(view)
    }

    /// Step 2 — the human approved: produce the proof to submit.
    ///
    /// The pending entry is removed only once the ceremony has actually produced a
    /// signature. A `RawAckRequired` refusal re-inserts the ceremony for a retry
    /// WITH the ack, so dropping our half here would strand that retry with no
    /// nonce.
    ///
    /// Returns the proof for the caller to POST. The signature never crosses the
    /// invoke bridge — the command layer consumes it and returns only
    /// [`WalletLinkResult`].
    pub fn approve(
        &self,
        vault: &CustodyVault,
        ceremony: &SignatureCeremony,
        id: &str,
        raw_ack: bool,
    ) -> Result<LinkProof, LinkError> {
        // Read (do not remove) so a ceremony that refuses can still be retried.
        let pending = self.lock().get(id).cloned().ok_or(LinkError::UnknownLink)?;

        let sig = ceremony
            .approve(vault, id, raw_ack)
            .map_err(|e| LinkError::Ceremony(e.to_string()))?;

        // Signed: this ceremony is spent either way now.
        self.lock().remove(id);

        Ok(LinkProof {
            address: pending.address,
            // The ceremony returns bare hex; the authority verifies an
            // 0x-prefixed EIP-191 signature.
            signature: format!("0x{}", sig.sig_hex),
            nonce: pending.nonce,
        })
    }

    /// Drop a pending link whose ceremony the human rejected.
    pub fn forget(&self, id: &str) {
        self.lock().remove(id);
    }

    /// How many links are awaiting approval (tests / diagnostics).
    pub fn pending_count(&self) -> usize {
        self.lock().len()
    }
}

// ---------------------------------------------------------------------------
// Tauri command surface
// ---------------------------------------------------------------------------

use tauri::State;

/// Managed state wrapper (mirrors `CeremonyState` / `AuthState`).
pub struct LinkState(pub WalletLinkState);

/// Build the managed state for a Tauri build.
pub fn build_link_state() -> LinkState {
    LinkState(WalletLinkState::new())
}

/// **Command — wallet_link_request.** Ask the authority for a one-time challenge
/// and open a ceremony over the exact message it will verify. Returns the
/// [`CeremonyView`] the approval UI renders — NEVER a signature (I-2).
///
/// Reads the wallet address first, so a locked vault fails before a one-time
/// nonce is spent.
#[tauri::command]
pub fn wallet_link_request(
    link: State<'_, LinkState>,
    auth: State<'_, crate::oidc::AuthState>,
    custody: State<'_, crate::custody::CustodyState>,
    ceremony: State<'_, crate::ceremony::CeremonyState>,
) -> std::result::Result<CeremonyView, String> {
    let info = crate::wallet::address(&custody.0).map_err(|e| e.to_string())?;
    let challenge = auth
        .0
        .wallet_link_challenge()
        .map_err(|e| LinkError::Authority(e.to_string()).to_string())?;
    link.0
        .open(
            &ceremony.0,
            crate::rpc::CITRATE_CHAIN_ID,
            auth.0.issuer(),
            &info.address,
            &challenge.nonce,
            &challenge.message_template,
        )
        .map_err(|e| e.to_string())
}

/// **Command — wallet_link_approve.** Consume the approved ceremony, then submit
/// the proof to the authority. Returns only the bound address — the signature is
/// consumed in-process and never crosses the bridge (I-2).
///
/// After this succeeds the authority serves this address as `wallet_address`, so
/// the membership money path pays an address this device can actually spend from.
#[tauri::command]
pub fn wallet_link_approve(
    link: State<'_, LinkState>,
    auth: State<'_, crate::oidc::AuthState>,
    custody: State<'_, crate::custody::CustodyState>,
    ceremony: State<'_, crate::ceremony::CeremonyState>,
    id: String,
    raw_ack: bool,
) -> std::result::Result<WalletLinkResult, String> {
    let proof = link
        .0
        .approve(&custody.0, &ceremony.0, &id, raw_ack)
        .map_err(|e| e.to_string())?;
    auth.0
        .wallet_link_submit(&proof.address, &proof.signature, &proof.nonce)
        .map_err(|e| LinkError::Authority(e.to_string()).to_string())?;

    // Make THIS device's wallet the canonical one — the address the authority
    // serves as `wallet_address`, and therefore the address the treasury pays.
    //
    // The authority defaults canonical to FIRST-linked so a stray second link can
    // never silently move a member's pay-to address. That protects against an
    // AUTOMATIC link; this one is neither automatic nor stray — the human just
    // approved a ceremony binding this specific wallet, and the desktop's own
    // invariant (`walletIsLinked`: claim == this device's custody address) says
    // this is the address that must be served. Without the promotion, a member
    // whose custody vault was replaced links successfully and stays blocked
    // forever, with no error anywhere (observed live 2026-08-04).
    //
    // Idempotent: on a first link the address is already canonical and the call
    // is a no-op at the authority.
    //
    // NOT fatal. The link is already durable and proven; failing the whole
    // command here would tell the member their wallet was not linked when it
    // was — the same reasoning the authority uses for its own canonical hook.
    // The outcome is REPORTED instead, so the UI can tell the truth rather than
    // claim a success the claim does not reflect.
    let canonical = match auth.0.wallet_set_canonical(&proof.address) {
        Ok(()) => true,
        Err(e) => {
            // Logged, never returned across the bridge: the authority's error
            // text is diagnostic, not something the UI should render.
            eprintln!(
                "[wallet-link] linked {} but could not make it canonical: {e}",
                proof.address
            );
            false
        }
    };

    Ok(WalletLinkResult {
        address: proof.address,
        linked: true,
        canonical,
    })
}

/// **Command — wallet_link_reject.** Drop a pending link the human declined.
#[tauri::command]
pub fn wallet_link_reject(
    link: State<'_, LinkState>,
    ceremony: State<'_, crate::ceremony::CeremonyState>,
    id: String,
) -> std::result::Result<(), String> {
    let _ = ceremony.0.reject(&id);
    link.0.forget(&id);
    Ok(())
}

#[cfg(test)]
mod tests {
    include!("wallet_link_tests.rs");
}
