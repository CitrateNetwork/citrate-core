---
created: 2026-10-01T00:00:00Z
branch: hup/n4-fleet-wizard
author: Larry Klosowski + Claude Opus 5.5
status: active (updated on hup/n5-fleet-rest: link codes, install link, deep link)
wp: HUP-S8.2, HUP-S8.3
---

# Fleet wizard and connectivity runbook (HUP-S8.2, HUP-S8.3)

US-8.1: "Connect my machines". The wizard lives on the Cluster surface. It checks this
machine, finds the member's other machines (only with consent), pairs them with a one-time
link or QR code, and helps with Tailscale when machines cannot reach each other.

Code: `src-tauri/src/fleet.rs` (commands, roster, pairing listener and client),
`fleet_pairing.rs` (tokens), `fleet_mdns.rs` (discovery), `fleet_tailscale.rs`
(detection and guidance); UI in `src/fleet/` and `src/bridge/tauri/fleet.ts`.

## What the member sees

1. **Start.** The local tier probe runs (the same probe as onboarding, `tier.rs`; no
   network). The wizard shows this machine's tier, the rationale, and a suggested role.
   The member can rename the machine; that name is what other machines see.
2. **Find machines (opt-in).** A checkbox, off by default, turns on mDNS discovery. While
   on, the app answers and asks for `_citrate-core._tcp.local` on the local network. It is
   off again when the wizard closes or the app restarts.
3. **Pair.** On the machine that already runs Citrate Core: "Create pairing link" shows a
   `citrate://pair?...` link and the same link as a QR code. On the new machine: paste the
   link, "Check link" (offline signature and expiry check), then "Pair with this machine".
   Both machines record each other with tier and role.
4. **Connectivity.** When pairing cannot reach the other machine, or discovery found
   nothing, the wizard reads Tailscale's status and shows guidance.
5. **Your machines.** This machine, paired machines and machines seen on the network, each
   with tier and role.

## Pairing link: properties

| Property | How |
|---|---|
| Signed | ed25519 over `"citrate/fleet-pair/v1\0" ‖ claim_json` with the app's pairing key. The new machine verifies against the key in the link; the issuing machine verifies against its own key, so a link minted elsewhere is unknown to it. |
| Short-lived | 10 minutes (`PAIR_TTL_SECS`, a default pending owner sign-off). A claim asking for longer is refused even when signed. 2 minutes of clock skew tolerated on `issued_at`. |
| Single use | The issuer keeps each open link's nonce; redeeming consumes it; a second use is "already used". At most 8 open links. |
| Not a wallet signature | The pairing key is generated in memory at app start, never written to disk, never derived from the wallet, and signs only pairing links (Rule 3). |

The link carries: a nonce, the pairing public key, the issuing machine's name and tier,
issue and expiry times, and up to 6 `ip:port` hints (this machine's LAN address and, when
Tailscale is connected, its tailnet IPv4 address).

**Wire protocol.** While any link is open the issuing machine listens on an ephemeral TCP
port on all interfaces. The new machine connects to each hint in turn (2.5 s each) and
sends one JSON line `{v, link, deviceId, label, tier, deviceLink?}`; the issuer answers one
JSON line `{ok, error, deviceId, label, tier, deviceLink?}`. Lines are capped at 8 KiB (4 KiB
before link codes travelled) with 5 s read/write deadlines. The listener closes by itself once
no link is open (all used or expired).

**DeviceLinks travel with the pairing (HUP-S8.1 follow-on, `hup/n5-fleet-rest`).** When a
machine is linked, its link code (the same public text "Copy link code" gives) rides in
`deviceLink`, both directions. The receiving machine verifies all three signatures and stores
the link only when it names the same member (`device_link::import_paired_code`); another
person's machine is reported, never added. Each paired machine's roster entry records the
outcome (`deviceLink`: added, otherMember, refused) and the wizard shows it ("linked under
you"). The pair step offers "Link this machine" (the wallet review, nothing moves) when this
machine is not linked yet. The issuing machine's list refreshes every 3 s while a link is open.

**Install link.** The pair step also shows `https://citrate.ai/download` and its QR code for a
machine without Citrate Core (the page picks the installer for the platform).

**Deep link.** `citrate://pair?...` opened from the OS lands on the Cluster screen with the
wizard on the pair step and the link filled in; nothing pairs until the member presses "Pair".

**Local roster.** `fleet.json` in the app data directory: this machine's random fleet id
(not a key), its name, and the paired machines (`id, label, tier, role, addr, pairedAt,
via`). An unreadable file is reported and never overwritten.

## Discovery: what is shared

An advert contains a random per-run instance id, the machine name the member typed, tier,
role, and the pairing port while a link is open (else 0). No wallet address, no comms
address, no computer name. The sender's IP comes from the packet. The codec is a small
PTR + TXT subset of DNS-SD on `std` + `socket2` (no mDNS library added).

## Tailscale: read-only

The app runs exactly `tailscale status --json` (4 s deadline), from `PATH` or the usual
install location (`/Applications/Tailscale.app/Contents/MacOS/Tailscale` on macOS,
`C:\Program Files\Tailscale\tailscale.exe` on Windows, `/usr/bin/tailscale` and friends on
Linux). It never runs `up`, `down`, `login` or `set`. States and the guidance shown:

| State | Guidance |
|---|---|
| Not installed / unreadable | Same network: check each machine's firewall allows Citrate Core. Different networks: install Tailscale on each machine, same account (link to tailscale.com/download). |
| Installed, not running / turned off / starting | Open Tailscale and turn it on on both machines, then create a new link. |
| Needs sign-in | Sign in with the same account on both machines, then create a new link. |
| Connected | Make sure the other machine is on the same tailnet and connected; a new link includes this machine's tailnet address. Offline tailnet machines are named. |

## Troubleshooting

- **macOS asks "accept incoming network connections?"** when a link is created: allow it,
  or pairing over the LAN cannot reach this machine. Tailscale addresses are subject to the
  same prompt.
- **"No other Citrate Core machines answered"**: the other machine needs discovery turned
  on too, and both must be on the same network segment (guest Wi-Fi and some office
  networks block multicast). Pairing by link does not need discovery.
- **"The other machine could not be reached"**: the wizard moves to the connectivity step.
  Check the firewall, or use Tailscale and create a new link (the old one still works until
  it expires, but it does not carry the tailnet address if Tailscale was off when it was made).
- **"already used" / "expired"**: create a new link on the issuing machine.
- **Clock warnings ("dated in the future")**: set both machines to network time.

## Not in these WPs

- QR scanning is by another device's camera app, which yields the link (the app has no camera
  scanner).
- Group creation and invites stay in Groups (the wizard points there).
- Cross-machine checks on Linux and Windows hardware (DGX ask on the S8 issue).

## Defaults pending owner sign-off

- Role names and tier mapping: T0 light, T1 worker, T2 heavy (`fleet::role_for`, `roleLabel`).
- Pairing link lifetime: 10 minutes (`PAIR_TTL_SECS`).
- Discovery consent not remembered across restarts.

## Tests

- Rust (`cd src-tauri; cargo test --lib fleet`): pairing tokens (signature, tamper, domain
  separation, expiry, future-dated, overlong lifetime, single use, cap, QR), mDNS codec
  (round trip, compression, pointer loops, truncation, foreign services, bad fields),
  Tailscale (parse, states, CLI failures, paths, guidance, read-only argv), and pairing end
  to end over loopback TCP (both rosters, single use over the wire, self-pairing refused,
  unreachable, junk on the port).
- `cargo test --lib fleet_mdns -- --ignored`: two real discovery instances find each other
  over multicast on one machine (ignored by default; CI sandboxes often block multicast).
- Vitest (`npx vitest run src/fleet`): wizard state machine and every view state.
