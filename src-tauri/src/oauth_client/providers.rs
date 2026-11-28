//! The compiled provider registry (L6-WP05). Public client ids only: an OAuth client id is not a
//! secret (it is in every authorize URL), so it is safe to embed, like `social.rs`'s Discord and
//! X ids. No client secret appears in this file or anywhere in a build.

use super::{ClientMode, ProviderSetup};
use crate::connections::Service;

/// Citrate Core's GitHub OAuth App (owner decision 2026-11-27: an OAuth App, not a GitHub App).
/// Public; its secret lives only in the relay deployment (`GITHUB_CLIENT_SECRET`).
pub const GITHUB_CLIENT_ID: &str = "Ov23liUgI4U92ZzDXwnt";

const NOTION_NOT_REGISTERED: &str = "the Notion public integration's client id is not yet \
     registered for release builds (docs/CONNECTIONS_RUNBOOK.md section 2)";
const GOOGLE_NOT_REGISTERED: &str = "no Google OAuth client is registered for release builds, \
     and the Citrate relay has no Google row yet (docs/CONNECTIONS_RUNBOOK.md section 2)";
const HF_NOT_REGISTERED: &str = "no Hugging Face OAuth app is registered for release builds, \
     and the Citrate relay has no Hugging Face row yet (docs/CONNECTIONS_RUNBOOK.md section 2)";

/// The registry row for one service.
pub(crate) fn setup_for(service: Service) -> ProviderSetup {
    match service {
        Service::GitHub => ProviderSetup {
            provider: "github",
            mode: ClientMode::Relay,
            client_id: Some(GITHUB_CLIENT_ID),
            configured: true,
            reason: None,
        },
        Service::Notion => ProviderSetup {
            provider: "notion",
            mode: ClientMode::Relay,
            client_id: None,
            configured: false,
            reason: Some(NOTION_NOT_REGISTERED),
        },
        Service::GoogleDrive | Service::GoogleSheets | Service::GoogleCalendar => ProviderSetup {
            provider: "google",
            mode: ClientMode::Relay,
            client_id: None,
            configured: false,
            reason: Some(GOOGLE_NOT_REGISTERED),
        },
        Service::HuggingFace => ProviderSetup {
            provider: "huggingface",
            mode: ClientMode::Relay,
            client_id: None,
            configured: false,
            reason: Some(HF_NOT_REGISTERED),
        },
    }
}

/// The registry row for a service id (`github`, `gdrive`, `notion`, `hf`, `gsheets`, `gcal`).
/// Never reads a credential file or the environment (frozen, lanes/L6 section 3.2).
pub fn provider_setup(provider: &str) -> Option<ProviderSetup> {
    Service::from_id(provider).map(setup_for)
}
