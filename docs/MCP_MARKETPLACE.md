---
created: 2026-11-27T19:45:00Z
branch: feat/v60-l6-wp02-characterization
author: Larry Klosowski + Claude Opus 5.5
status: active (characterization suite only; the Marketplace itself is not built yet)
wp: L6-WP02
---

# MCP Marketplace (0.6.0 lane L6)

This page belongs to lane L6 (Connections and Marketplace) of the 0.6.0 planset
(`.agentile/planset/2026-11-07-v060/lanes/L6_CONNECTIONS_MARKETPLACE.md` in citrate-federation).
Today it holds one thing: the lane's characterization suite. The Marketplace sections (pinned
brand servers, the signed subregistry, search, install, the dashboard) are added by the work
packages that build them (L6-WP07 to WP12). Until then, user-added servers are described in
[MCP_USER_SERVERS.md](MCP_USER_SERVERS.md) and the Store in [STORE_CATALOG.md](STORE_CATALOG.md).

## Verified brand servers (L6-WP07: implemented, IPC wired, UI in L6-WP11)

`src-tauri/src/lanes/l6/mcp_catalog/brand.rs` holds the brand servers compiled into the app.
They are listed with the "Verified brand" badge whether or not the catalog key exists, so the
Marketplace works before the key ceremony. `mcp_catalog_list` returns them with `signed: false`
and the refusal `no_key`, "Signed catalog not available yet: signing key not issued", for as long
as the catalog key slot is empty. A filled slot alone does not change `signed`: only a verified
signed subregistry can (L6-WP08c).

Verification procedure for a row (the evidence is the provider matrix, citrate-federation
`.agentile/planset/2026-11-07-v060/evidence/L6-WP07_MCP_PROVIDER_MATRIX_2026-11-28.md`):

1. Domain match: the endpoint host is on the brand's own domain, and the source is the
   publisher's own page.
2. Probe: an unauthenticated `initialize` answers 401 with a `resource_metadata` challenge (or
   200 for a server without sign-in), and the protected-resource and authorization-server
   metadata are read.
3. Registration: CIMD or DCR is advertised, so the app can sign in without a registration the
   owner has not made yet.
4. Date: the day a person checked the row goes in `checked`.

Only rows with status `supported` are pinned: 13 today (Notion, Linear read-only, Vercel,
Atlassian v2, Hugging Face, Stripe, Cloudflare API, Cloudflare Workers Bindings, Cloudflare
Observability, Cloudflare Docs, Sentry, Supabase, Neon), all checked 2026-11-28. Left out until
their blocker clears: Slack, Gmail, Drive, Figma, Asana and HubSpot (publisher approval or owner
registration), GitHub (citrate-core GitHub App, L6-WP13) and X (redirect registration), PayPal and
Discord (unverified), LinkedIn (no server) and Microsoft Work IQ (deferred). The lane file's floor
of 20 entries is not met: an entry that cannot be verified is left out, never guessed. Write tools
are off by default on every row. Tests: `cargo test -p citrate-core --lib mcp_catalog`.

## Characterization suite

The suite pins what Connections, user-added MCP servers and the Store guarantee at citrate-core
`a826e441`, before any L6 work package changes the code around them. It is green on main. A test
goes red only when a later change regresses the guarantee it names; a work package that changes a
pinned behavior on purpose names the test it supersedes and replaces the assertion only with a
stricter one, in a commit that says why.

Run it alone:

```sh
cargo test -p citrate-core --lib l6_char
```

The registry [`ci/characterization.d/l6.json`](../ci/characterization.d/l6.json) names each suite,
its cargo filter and its tests. Nothing in citrate-core reads `ci/characterization.d/` yet; LQ wires
the registries into the merge-queue run, and until then the suite runs as part of the ordinary
`cargo test` job. The registry, this list and the suite files must name the same tests, and each
filter must select the module test block that includes the suite file
(`char_suite_is_listed_for_lq` checks both).

### L6-CH-1 Connections (`src-tauri/src/lanes/l6/characterization/connections_char_tests.rs`)

- `char_no_command_returns_a_token`: the connections commands return a status or nothing, and no
  serialized status names or carries a token on any path.
- `char_release_without_creds_reports_not_configured`: with no client credentials, connect is
  `NotConfigured` before the callback port is bound or a browser opened.
- `char_state_mismatch_fails_closed`: drive-by and forged callbacks never burn the sign-in; a
  callback without a code fails closed.
- `char_slot_names_are_connection_prefixed`: vault slots are `connection-<service>` and only the
  backend writes them.
- `char_expired_token_never_sent`: an absent, locked, expired or empty token is never handed out.
- `char_suite_is_listed_for_lq`: the registry, this page and the suite files agree.

### L6-CH-2 User-added MCP servers (`src-tauri/src/lanes/l6/characterization/mcp_servers_char_tests.rs`)

- `char_user_server_saved_disabled_and_needs_probe`: a saved server is disabled until a probe of
  the entry as it stands succeeds; an edit after a successful probe, or turning the server off,
  needs a new probe before it is enabled.
- `char_probe_claiming_trusted_tool_refused`: a probe report that claims a trusted tool is refused.
- `char_env_values_only_in_keychain`: env values live in the OS keychain and never in the saved
  server list (`mcp-servers.json`). They still leave the keychain to run a server, into the
  private probe file during a check and into the spawn-time allowlist at Hermes start (see
  [MCP_USER_SERVERS.md](MCP_USER_SERVERS.md), Files); this test does not pin those.
- `char_max_servers_is_16`: at most 16 servers, the runtime's limit.
- `char_hermes_node_write_tools_off`: Hermes is offered the node server's read tools only, pending
  the owner's sign-off.

### L6-CH-3 Store (`src-tauri/src/lanes/l6/characterization/store_char_tests.rs`)

- `char_store_no_key_refuses_before_network`: the shipped catalog key slot is empty, and an empty
  slot refuses before any fetch or write.
- `char_store_first_party_only`: a catalog with a third-party listing is refused even with a good
  signature; nothing is shown, kept or stageable from it.
