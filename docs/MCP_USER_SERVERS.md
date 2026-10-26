---
created: 2026-10-01T12:00:00Z
branch: hup/n4-user-mcp
author: Larry Klosowski + Claude Opus 5.5
status: implemented (core + runtime); end-to-end run in the packaged app not yet recorded
wp: HUP-S4.4
updated: 2026-10-26 (rc52/prov-mcp, WP-S1-PROV-MCP, ADR-2026-10-26 P-9a: env values in the OS keychain)
---

# User-added MCP servers (Settings > MCP servers)

HUP-S4.4, planset `2026-09-30-hermes-upskill`, US-4.1 AC1 ("user-added servers,
allowlisted, with a review step"). A member can give Hermes tools from their own MCP
servers. Nothing a member adds reaches Hermes until they have checked it and looked
over what it offers.

## Flow

1. **Add or edit** (Settings > MCP servers). Either a program on this computer
   (stdio: absolute path, arguments one per line, optional working folder, explicit
   environment variables) or a URL (https, or http to this computer). The entry is
   validated and stored **off**. Any change to what runs (program, arguments, folder,
   URL, an env value, the write-tools switch) turns a server off until it is reviewed
   again; a save that changes nothing keeps it on.
2. **Check and review.** Core asks the running Hermes sidecar for a dry-run probe
   (`POST /mcp/probe`): it starts or reaches the server, runs `initialize` and
   `tools/list`, and stops it. Nothing is registered. The review screen shows:
   - the exact command line (or URL) and working folder;
   - the env keys, with values masked (the mask says nothing about the value);
   - the server's name, version and protocol version;
   - every tool with its annotations as badges (`read-only` or `writes`,
     `destructive`, `idempotent`, `open world`) and `untrusted`, and whether Hermes
     would be offered it (with the reason when not).
3. **Turn on.** Allowed only with the review token from a successful check of the
   entry exactly as it stands (checked in Rust, not just in the UI). It takes effect
   the next time Hermes starts, when core writes the allowlist the sidecar reads.

The sidecar must be running for a check ("Start Hermes to check a server" otherwise).

## Safety properties

| Property | Where it is enforced |
|---|---|
| Every MCP tool is untrusted; its output taints the session | runtime `agent-mcp-host` mapping (trust is always `untrusted`); core refuses to record a check whose report claims otherwise |
| Write tools are not offered unless turned on per server | runtime host offer decision (shared with the probe, so the review shows exactly what a session gets) |
| No inherited secrets | stdio servers start with a cleared environment plus the process basics (`PATH`, `HOME`, locale, temp dir) and the entry's explicit values only |
| Env values are explicit | `$X`, `${X}` and `%X%` values are refused (nothing is expanded) |
| The review screen shows the code that runs | env names that change what a process loads (`LD_*`, `DYLD_*`, `NODE_OPTIONS`, `PYTHONPATH`, `BASH_ENV`, ...) are refused |
| Built-in names are reserved | `mem`, `citrate-node`, `scan`, `browser`, `toolchain`, ... cannot be used |
| Review binds to the exact entry | the enable token is a salted hash of the entry's fingerprint (command, args, folder, URL, env names and keychain references, write switch); a new value is a new keychain item with a new reference, so an edit, a new value or a rename needs a new check |
| Env values live in the OS keychain | service `ai.citrate.core`, one item per value, account `mcp-server-env:<server>:<ENV_NAME>:<reference>`; the server list holds names and references only |
| Env values never return to the webview | views carry masks only; an edit that does not retype a value keeps the stored one |
| Env values leave the keychain only to run a server | at Hermes start (the spawn-time allowlist) and during a check (the probe request and the probe file); see Files |
| Nothing changes for members who never use this | with no enabled server no allowlist is written and `CITRATE_HERMES_MCP` is not set |
| Keyless (Rule 3) | no key or signature anywhere in this path |

Validation runs twice: in core (so the form can show errors without the sidecar) and
in the runtime (`agent-mcp-host::user`, the source of truth), which re-checks every
probe with the full user-entry rules. When the sidecar loads the allowlist file at
start it applies the allowlist's base rules (types, absolute command, URL scheme,
limits) and fails closed on an invalid file; the user-only rules (reserved names,
loader env names, env references) are applied by core when it writes the file and
by the probe, not again at load.

## Files

Under `<app local data>/hermes/`, all written `0600` (atomically: a synced temp file
renamed over the old one):

- `mcp-servers.json`: every entry, including those that are off. **No env values**: each
  env name maps to a reference to its keychain item.
- `mcp-effective.json`: the one allowlist the sidecar loads (built-in servers first, then
  the member's enabled, reviewed servers with their env values from the keychain). Written
  right before Hermes is spawned; removed as soon as the sidecar answers `/health` (it
  loads the allowlist before it serves). It is written again right before each
  supervisor restart and removed whenever a child ends, including a failed spawn and the
  last crash before the supervisor gives up, when Hermes stops, and at app start (with the
  `.tmp` file an interrupted atomic write can leave). With only built-in servers on, the
  built-in file is passed as is.
- `mcp-probe.json`: exactly the entry being checked, with its values, for the length of
  one check (`CITRATE_HERMES_MCP_REGISTRY` names it; the sidecar starts only an entry that
  matches it).

### Migration from plain text (RC52)

Versions before RC52 kept env values in plain text in `mcp-servers.json` and
`mcp-allowlist.json`. The first time the list is read (at app start, in the background,
or when Settings or Hermes reads it), core writes each value to the keychain, then
rewrites `mcp-servers.json` without them, then removes `mcp-allowlist.json`. If a
keychain write or the rewrite fails, the items that attempt wrote are removed and the
file is left as it was, so the next read tries again; a rerun after success changes
nothing. A server that was on and reviewed stays on. "Delete my local data" lists and
deletes these keychain items.

**Uninstalling by dragging the app to the Trash does not remove keychain items.** Use
Settings > "Delete my local data" first, or remove the `ai.citrate.core` items whose
account starts with `mcp-server-env:` in Keychain Access. An item written just before the
app was killed, and never referenced by the saved list, is not listed by "Delete my local
data" (the keychain library cannot enumerate items by prefix); it is never read, and can
be removed the same way.

## Commands

`mcp_servers_list`, `mcp_server_save`, `mcp_server_remove`, `mcp_server_disable`,
`mcp_server_review` (45 s webview deadline: the probe is bounded at 20 s in the
sidecar), `mcp_server_enable`, `mcp_servers_runtime` (what the running sidecar
loaded). All async (`blocking::off_main`), all in the main-window ACL only.

## Owner decisions (pending owner sign-off)

These are conservative placeholders. They refuse more than they allow and change
nothing for members who never add a server.

- **Reserved server names** (`RESERVED_SERVER_NAMES` in runtime
  `agent-mcp-host/src/user.rs`, mirrored in core `src-tauri/src/mcp_servers.rs`),
  chosen from planset 02 §5.
- **Loader env denylist** (`LD_*`, `DYLD_*`, `NODE_OPTIONS`, `PYTHONPATH`, ...). It
  refuses some legitimate uses, such as `NODE_OPTIONS` for memory flags; pass those
  as program arguments instead.
- **Env values in the OS keychain** (decided 2026-10-26, ADR-2026-10-26 P-9a): handed to
  the sidecar only at spawn and during a check, as above. The sidecar still reads them from
  a `0600` file for that moment, because it reads its MCP allowlist from a file; a hand-off
  over the bearer-authenticated loopback would remove even that window and needs a runtime
  change.
- **Checking needs Hermes running**, and changes apply at the next Hermes start.
- **Masks show nothing** about the value, not even its length (the list holds no value
  to measure).

## Not done

- Not yet exercised end to end in the packaged app (sidecar + a real third-party MCP
  server); the runtime probe is tested against real stdio and HTTP MCP servers, and
  core is tested with a mock control transport.
- Changes apply at the next Hermes start; there is no live reload of MCP servers.
- OAuth for remote MCP servers, MCP resources and prompts are out of scope (runtime
  README, "Not implemented").
