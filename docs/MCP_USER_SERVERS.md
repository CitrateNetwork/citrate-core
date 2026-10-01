---
created: 2026-10-01T12:00:00Z
branch: hup/n4-user-mcp
author: Larry Klosowski + Claude Opus 5.5
status: implemented (core + runtime); end-to-end run in the packaged app not yet recorded
wp: HUP-S4.4
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
   - the env keys, with values masked (length only);
   - the server's name, version and protocol version;
   - every tool with its annotations as badges (`read-only` or `writes`,
     `destructive`, `idempotent`, `open world`) and `untrusted`, and whether Hermes
     would be offered it (with the reason when not).
3. **Turn on.** Allowed only with the review token from a successful check of the
   entry exactly as it stands (checked in Rust, not just in the UI). Core then writes
   the allowlist file the sidecar reads. It takes effect the next time Hermes starts.

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
| Review binds to the exact entry | the enable token is a salted hash of the entry's fingerprint (command, args, folder, URL, env keys and values, write switch); an edit or a rename needs a new check |
| Env values never return to the webview | views carry masks only; an edit that does not retype a value keeps the stored one |
| Nothing changes for members who never use this | with no enabled server the allowlist file does not exist and `CITRATE_HERMES_MCP` is not set |
| Keyless (Rule 3) | no key or signature anywhere in this path |

Validation runs twice: in core (so the form can show errors without the sidecar) and
in the runtime (`agent-mcp-host::user`, the source of truth), which re-checks every
probe and the allowlist on load and fails closed.

## Files

Under `<app local data>/hermes/`, both written `0600`:

- `mcp-servers.json`: every entry, including those that are off. Holds env values
  (often tokens) in plain text, protected by file permissions, because the sidecar
  needs them to start the server and holds no keyring access.
- `mcp-allowlist.json`: enabled, reviewed entries only, in the runtime's
  `[[servers]]` shape. Removed when nothing is enabled.

## Commands

`mcp_servers_list`, `mcp_server_save`, `mcp_server_remove`, `mcp_server_disable`,
`mcp_server_review` (45 s webview deadline: the probe is bounded at 20 s in the
sidecar), `mcp_server_enable`, `mcp_servers_runtime` (what the running sidecar
loaded). All async (`blocking::off_main`), all in the main-window ACL only.

## Not done

- Not yet exercised end to end in the packaged app (sidecar + a real third-party MCP
  server); the runtime probe is tested against real stdio and HTTP MCP servers, and
  core is tested with a mock control transport.
- Changes apply at the next Hermes start; there is no live reload of MCP servers.
- OAuth for remote MCP servers, MCP resources and prompts are out of scope (runtime
  README, "Not implemented").
