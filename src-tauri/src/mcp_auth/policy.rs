//! Local tool policy for remote MCP servers (L6-WP06, owner brief 2026-11-28): write tools are
//! off by default, whatever scope the token carries. The MCP gateway (L1) asks [`decide`] before
//! offering or calling a tool; a member turns writes on per server (the server entry's
//! `allow_write_tools`, `mcp_servers.rs`, default `false`).
//!
//! A tool is a read only when the evidence says so:
//! - a provider with a tool list in the L6-WP07 matrix (live anonymous `tools/list` for Gmail,
//!   Drive and Hugging Face; the publisher's docs for Notion and Stripe): listed reads are reads,
//!   anything else (listed write, or not listed) is a write;
//! - any other server: `readOnlyHint: true` without `destructiveHint: true`.
//!
//! `destructiveHint: true` always makes a write, and a tool with no annotations is a write.

use crate::mcp_servers::ProbeHints;

/// Read or write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ToolClass {
    Read,
    Write,
}

/// Whether the gateway may offer and call a tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Decision {
    Allow,
    /// Blocked, with the reason the member sees.
    Blocked(&'static str),
}

/// The member-facing reason a write is blocked.
pub(crate) const WRITES_OFF: &str =
    "write tools are off for this server; turn on writes for it in Marketplace to allow them";

/// One provider's tool lists (matrix 2026-11-28).
pub(crate) struct ProviderTools {
    pub provider: &'static str,
    pub reads: &'static [&'static str],
    pub writes: &'static [&'static str],
}

/// The providers whose tool names the matrix recorded.
pub(crate) const PROVIDER_TOOLS: &[ProviderTools] = &[
    ProviderTools {
        provider: "gmail",
        reads: &[
            "list_drafts",
            "get_draft",
            "get_thread",
            "get_message",
            "search_threads",
            "list_labels",
            "list_filters",
        ],
        writes: &[
            "create_draft",
            "update_draft",
            "send_message",
            "reply",
            "forward",
            "label_thread",
            "unlabel_thread",
            "apply_sensitive_thread_label",
            "trash_thread",
            "untrash_thread",
            "mark_thread_spam",
            "unmark_thread_spam",
            "label_message",
            "update_message_labels",
            "unlabel_message",
            "apply_sensitive_message_label",
            "trash_message",
            "untrash_message",
            "mark_message_spam",
            "unmark_message_spam",
            "create_label",
            "create_filter",
            "update_label",
        ],
    },
    ProviderTools {
        provider: "gdrive",
        reads: &[
            "download_file_content",
            "get_file_metadata",
            "get_file_permissions",
            "get_shared_drive_metadata",
            "list_file_revisions",
            "list_recent_files",
            "list_shared_drives",
            "read_file_content",
            "search_files",
        ],
        writes: &[
            "copy_file",
            "create_file",
            "request_access",
            "trash_file",
            "update_file",
            "upload_file_revision",
        ],
    },
    ProviderTools {
        provider: "huggingface",
        reads: &["hf_whoami", "hub_repo_search", "hub_repo_details", "hf_fs"],
        writes: &[],
    },
    ProviderTools {
        provider: "notion",
        reads: &[
            "notion-search",
            "notion-connector-search",
            "notion-query-data-sources",
        ],
        writes: &[
            "notion-create-pages",
            "notion-update-page",
            "notion-create-comment",
            "notion-create-file-upload",
        ],
    },
    ProviderTools {
        provider: "stripe",
        reads: &[
            "stripe_api_search",
            "stripe_api_details",
            "stripe_api_read",
            "get_stripe_account_info",
            "search_stripe_documentation",
            "get_balance_summary",
        ],
        writes: &[
            "stripe_api_write",
            "send_money",
            "transfer_money",
            "convert_currency",
            "setup_recipient",
            "atlas_company_formation",
        ],
    },
];

/// Classify `tool` of `provider` (`None` for a server with no matrix tool list).
pub(crate) fn classify(provider: Option<&str>, tool: &str, hints: &ProbeHints) -> ToolClass {
    if hints.destructive_hint == Some(true) {
        return ToolClass::Write;
    }
    if let Some(row) = provider.and_then(|p| PROVIDER_TOOLS.iter().find(|r| r.provider == p)) {
        return if row.reads.contains(&tool) && !row.writes.contains(&tool) {
            ToolClass::Read
        } else {
            ToolClass::Write
        };
    }
    if hints.read_only_hint == Some(true) {
        ToolClass::Read
    } else {
        ToolClass::Write
    }
}

/// Whether the gateway may offer and call `tool`. `writes_enabled` is the member's per-server
/// switch; the token's scope plays no part.
pub(crate) fn decide(
    provider: Option<&str>,
    tool: &str,
    hints: &ProbeHints,
    writes_enabled: bool,
) -> Decision {
    match classify(provider, tool, hints) {
        ToolClass::Read => Decision::Allow,
        ToolClass::Write if writes_enabled => Decision::Allow,
        ToolClass::Write => Decision::Blocked(WRITES_OFF),
    }
}

/// The member-facing reason a tool is unavailable on a limited connector.
pub(crate) const SCOPE_NOT_GRANTED: &str =
    "this tool needs a permission you did not grant; reconnect the server to grant it";

/// [`decide`] for a limited connector (lead decision 2026-11-28): a tool that needs a scope the
/// server did not grant is unavailable, whatever else applies. `required` is the tool's scopes as
/// the gateway knows them (a publisher's tool-to-scope list); `missing` is the sealed record's
/// `missing_scopes`. A tool with no known scope requirement falls through to [`decide`], and a
/// call the server then refuses with `insufficient_scope` goes to the step-up notice.
pub(crate) fn decide_scoped(
    provider: Option<&str>,
    tool: &str,
    hints: &ProbeHints,
    writes_enabled: bool,
    required: &[&str],
    missing: &[String],
) -> Decision {
    if required.iter().any(|r| missing.iter().any(|m| m == r)) {
        return Decision::Blocked(SCOPE_NOT_GRANTED);
    }
    decide(provider, tool, hints, writes_enabled)
}
