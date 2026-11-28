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

## Signed subregistry (L6-WP08a: verifier implemented, signing key not issued)

The Citrate subregistry is a curated mirror of the official MCP Registry
(`registry.modelcontextprotocol.io`). It is verified by `components/src/mcp_subregistry.rs` and
tested by `components/tests/mcp_subregistry.rs` (`cargo test -p citrate-components --test
mcp_subregistry`).

- **Key.** The document is signed offline with the catalog key, the same key and custody as the
  Store catalog, so the trust root is the `PRODUCTION_CATALOG_PUBKEY` slot. That slot is empty
  until the owner's key ceremony. Until then every document is refused with "Signed catalog not
  available yet: signing key not issued", and the Marketplace lists only the verified brand
  servers compiled into the app (L6-WP07). There is no placeholder key: a test scans the verifier
  source for any minisign public key line.
- **Domain.** The trusted comment must start with `citrate-mcp-subregistry-v1` (followed by
  nothing, a space or a tab). A Store catalog signature (`citrate-store-catalog`) or a component
  manifest signature over the same bytes is refused as the wrong purpose, and a subregistry
  signature never passes as a Store catalog.
- **Order of checks.** Size cap (2 MiB) before the signature and before parsing, then the
  signature (prehashed Ed25519 only), then the JSON and field rules, then freshness (at most 31
  days of validity, 10 minutes of clock skew) and the sequence.
- **Rollback.** A document with a lower sequence than the last one accepted, or the same sequence
  with different bytes, is refused against the last-seen record the caller passes in.
- **Schema v1.** `{ schema, sequence, issued_at, expires_at, servers[] }`. Each server follows the
  official `server.json` (`name` as `<reverse-dns>/<server>`, `title`, `description`, `version`,
  `remotes[]` of `{ type: "streamable-http" | "sse", url }`, `packages[]` of `{ registryType:
  "npm" | "pypi" | "oci", identifier, version }`) plus `_meta["ai.citrate/curation"] = { trust:
  "verified_brand" | "curated", reviewedAt: "YYYY-MM-DD", toolSnapshotSha256? }`. Third-party
  servers are allowed, unlike the Store catalog, but only with that curation record. Every remote
  must be https with a host; every package must be pinned to an exact version (never `latest` or
  a range). Unknown fields or trust values, and any entry that breaks a rule, refuse the whole
  document.

What waits on the ceremony: the production key in the slot, the first signed document on the CDN,
and with them every runtime proof of the signed path (L6-WP08c).
