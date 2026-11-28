//! L6-WP07: the verified brand MCP servers compiled into the app. They are listed before the
//! catalog key exists, so the Marketplace works with no signing key at all.
//!
//! Every row is a `supported` row of the provider matrix (citrate-federation
//! `.agentile/planset/2026-11-07-v060/evidence/L6-WP07_MCP_PROVIDER_MATRIX_2026-11-28.md`): the
//! endpoint answered an unauthenticated probe, its protected-resource and authorization-server
//! metadata were read, and a registration path the app can use today (CIMD or DCR) is advertised.
//! Rows that need a publisher approval or an owner registration first (Slack, GitHub, Google,
//! Figma, Asana, HubSpot, X), unverified rows (PayPal, Discord), LinkedIn (no server) and
//! Microsoft Work IQ (deferred) are left out, never guessed. The verification procedure is in
//! docs/MCP_MARKETPLACE.md.
//!
//! A pinned row can be overridden only by a newer record in a verified signed subregistry
//! (L6-WP08c). Per-server quirks (resource URL, issuer slash, path-inserted metadata) are data for
//! L6-WP06, never code branches.

/// How the server is signed in to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Auth {
    Oauth,
    /// The server answers without sign-in.
    None,
}

impl Auth {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Auth::Oauth => "oauth",
            Auth::None => "none",
        }
    }
}

/// How the app registers as an OAuth client (L6-WP06 order: CIMD, then DCR).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Client {
    /// Client ID Metadata Document.
    Cimd,
    /// CIMD advertised, but the token endpoint may refuse a public client: fall back to DCR on
    /// `invalid_client` (matrix quirk 5).
    CimdThenDcr,
    /// Dynamic Client Registration (registered once per issuer).
    Dcr,
    /// No sign-in.
    NotNeeded,
}

impl Client {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Client::Cimd => "cimd",
            Client::CimdThenDcr => "cimd_then_dcr",
            Client::Dcr => "dcr",
            Client::NotNeeded => "none",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct BrandServer {
    /// Also the user-server name on install: 1-24 of a-z, 0-9, `-`.
    pub(crate) id: &'static str,
    pub(crate) name: &'static str,
    pub(crate) publisher: &'static str,
    pub(crate) summary: &'static str,
    /// The Streamable HTTP endpoint, exactly as probed.
    pub(crate) url: &'static str,
    pub(crate) auth: Auth,
    pub(crate) client: Client,
    /// Write tools stay off until the member turns them on in the review step.
    pub(crate) write_tools_default: bool,
    /// The date a person checked the row (YYYY-MM-DD).
    pub(crate) checked: &'static str,
    /// The publisher's own page for the server.
    pub(crate) source: &'static str,
}

const CHECKED: &str = "2026-11-28";

const fn oauth(
    id: &'static str,
    name: &'static str,
    publisher: &'static str,
    summary: &'static str,
    url: &'static str,
    client: Client,
    source: &'static str,
) -> BrandServer {
    BrandServer {
        id,
        name,
        publisher,
        summary,
        url,
        auth: Auth::Oauth,
        client,
        write_tools_default: false,
        checked: CHECKED,
        source,
    }
}

const CF_DOCS: &str =
    "https://developers.cloudflare.com/agents/model-context-protocol/mcp-servers-for-cloudflare/";

pub(crate) const BRAND_SERVERS: &[BrandServer] = &[
    oauth(
        "notion",
        "Notion",
        "Notion Labs",
        "Search, read and create pages and databases in your Notion workspace.",
        "https://mcp.notion.com/mcp",
        Client::Cimd,
        "https://developers.notion.com/docs/get-started-with-mcp",
    ),
    oauth(
        "linear",
        "Linear",
        "Linear",
        "Find and read issues, projects and comments in Linear (read-only endpoint).",
        "https://mcp.linear.app/mcp/readonly",
        Client::Cimd,
        "https://linear.app/docs/mcp",
    ),
    oauth(
        "vercel",
        "Vercel",
        "Vercel",
        "Search the Vercel docs and read teams, projects, deployments and build logs.",
        "https://mcp.vercel.com",
        Client::Cimd,
        "https://vercel.com/docs/mcp/vercel-mcp",
    ),
    oauth(
        "atlassian",
        "Atlassian (Jira and Confluence)",
        "Atlassian",
        "Search and read Jira issues and Confluence pages. Your org admin may need to allow it.",
        "https://mcp.atlassian.com/v2/mcp",
        Client::Cimd,
        "https://support.atlassian.com/atlassian-rovo-mcp-server/docs/getting-started-with-the-atlassian-remote-mcp-server/",
    ),
    oauth(
        "huggingface",
        "Hugging Face",
        "Hugging Face",
        "Search and read models, datasets and Spaces on the Hugging Face Hub.",
        "https://huggingface.co/mcp",
        Client::CimdThenDcr,
        "https://huggingface.co/docs/hub/hf-mcp-server",
    ),
    oauth(
        "stripe",
        "Stripe",
        "Stripe",
        "Search the Stripe docs and read your Stripe account, balance and API objects.",
        "https://mcp.stripe.com",
        Client::Dcr,
        "https://docs.stripe.com/mcp",
    ),
    oauth(
        "cloudflare",
        "Cloudflare API",
        "Cloudflare",
        "Read and manage your Cloudflare account through the Cloudflare API.",
        "https://mcp.cloudflare.com/mcp",
        Client::Cimd,
        CF_DOCS,
    ),
    oauth(
        "cloudflare-bindings",
        "Cloudflare Workers Bindings",
        "Cloudflare",
        "Work with Workers storage, AI and compute bindings in your Cloudflare account.",
        "https://bindings.mcp.cloudflare.com/mcp",
        Client::Dcr,
        CF_DOCS,
    ),
    oauth(
        "cloudflare-observability",
        "Cloudflare Observability",
        "Cloudflare",
        "Read Workers logs and analytics to debug your Cloudflare applications.",
        "https://observability.mcp.cloudflare.com/mcp",
        Client::Dcr,
        CF_DOCS,
    ),
    BrandServer {
        id: "cloudflare-docs",
        name: "Cloudflare Docs",
        publisher: "Cloudflare",
        summary: "Search the Cloudflare developer documentation. No sign-in needed.",
        url: "https://docs.mcp.cloudflare.com/mcp",
        auth: Auth::None,
        client: Client::NotNeeded,
        write_tools_default: false,
        checked: CHECKED,
        source: CF_DOCS,
    },
    oauth(
        "sentry",
        "Sentry",
        "Sentry",
        "Read issues, errors and projects in Sentry. Its default sign-in asks for write scopes.",
        "https://mcp.sentry.dev/mcp",
        Client::Cimd,
        "https://docs.sentry.io/product/sentry-mcp/",
    ),
    oauth(
        "supabase",
        "Supabase",
        "Supabase",
        "Read and manage your Supabase projects, databases and storage.",
        "https://mcp.supabase.com/mcp",
        Client::Dcr,
        "https://mcp.supabase.com/api/mcp",
    ),
    oauth(
        "neon",
        "Neon",
        "Neon",
        "Read and manage your Neon Postgres projects, branches and databases.",
        "https://mcp.neon.tech/mcp",
        Client::Cimd,
        "https://neon.com/docs/ai/neon-mcp-server",
    ),
];
