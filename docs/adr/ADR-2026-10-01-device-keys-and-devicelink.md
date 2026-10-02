---
created: 2026-10-01
branch: hup/n4-devicelink
author: Larry Klosowski + Claude Opus 5.5
status: accepted (HUP-S8.1); cross-machine use stays behind the CL-S4 transport sign-off (HUP-S8.4)
wp: HUP-S8.1
amends: docs/adr/ADR-2026-08-30-cluster-identity-and-transport.md (the Noise seed clause)
companions:
  - ../citrate-cluster/.agentile/planset/adr/ADR-002-per-device-keys-and-devicelink.md (verification + admission)
  - ../citrate-cluster/formal/DeviceLink.tla (TLC model)
  - .agentile/planset/2026-09-30-hermes-upskill/02_ARCHITECTURE.md section 9 (D-31)
---

# Per-device keys and the DeviceLink

## Context

The cluster mesh identifies a peer by its secp256k1 key. ADR-2026-08-30 made the cluster identity
the comms identity, and CONNECT-S5 then derived the comms key from the wallet so membership follows
a person across installs. Together those mean every machine a member runs presents the same key and
the same libp2p PeerId. The mesh cannot tell the machines apart and cannot remove one without the
others. Planset decision D-31 (owner amendment 2026-09-30): a random device key per machine plus a
wallet-signed DeviceLink.

## Decision

1. **Device key.** Each machine mints a random secp256k1 key on first link and seals it in the OS
   keyring (`cluster-device-key-v1`). It is never derived from the wallet and carries no value. It
   reaches the cluster daemon only as its 0600 seed file, where it is the Noise / libp2p identity.
2. **DeviceLink.** One human-readable message, `Citrate DeviceLink v1 ... member / device / wallet /
   index / label / issued_at`, signed by three keys:
   * the **wallet**, through the SignatureCeremony (the person sees the exact text at the review gate
     and approves it; `device_link_request` opens the ceremony and signs nothing);
   * the **member** (comms) key, the identity the roster and the mesh check;
   * the **device** key, as proof that this machine holds the key the link names.
   The member and device signatures are made in core, in the approve step, only after the person
   approved the wallet signature. Both are non-value scoped keys core already holds (the comms key
   already signs MLS traffic in the member daemon); the wallet key is only ever used by the ceremony.
3. **Store.** Links and revocations live in `<app data>/cluster/device-links.json` (owner-only,
   atomic writes). Every roster update sends them to the cluster daemon, which verifies all three
   signatures and adds each allowed member's devices to the mesh with the member's role.
4. **Revocation.** The member's comms key signs a revocation; the daemon honours it permanently for
   that device key and evicts in the same roster update. Revoking this machine also deletes its
   device key, so linking it again mints a fresh one. Revocation is two steps in core:
   `device_link_revoke_prepare` returns the statement the member confirms and a one-shot id for
   that device, and `device_link_revoke` takes only that id (added 2026-10-01).
   **Limit (added 2026-10-01).** Revocation removes a device key, not the member: the comms key is
   derived from the wallet and stays admitted beside linked devices, so a revoked machine that still
   holds the wallet can rejoin as the member's comms identity. The Cluster surface says so. Closing
   that needs the mesh to stop admitting the comms identity for members with linked devices (or a
   comms key rotation on revocation), which is cluster-side work.
5. **Defaults change nothing.** The mesh uses the device key only when the operator has turned on
   the cross-machine transport (`CITRATE_CLUSTER_LISTEN`, soak-gated) AND this machine has an active
   link. Otherwise the cluster meshes as the comms identity exactly as before. The identity is chosen
   when the cluster daemon starts, so a new link takes effect after Citrate Core restarts.
6. **Your own devices only, for now.** `device_link_export` gives this machine's signed link as a
   code, and `device_link_import` accepts a code for another device of the SAME member (verified
   before it is stored). Spreading links between different members rides on the comms roster in a
   follow-on; until then a node admits the devices whose links its own store holds.

## Commands (main window only; pop-outs unchanged)

`device_link_request`, `device_link_approve`, `device_link_reject`, `device_links`,
`device_link_revoke_prepare`, `device_link_revoke`, `device_link_export`, `device_link_import`, `cluster_devices`. All async on the
blocking pool (keyring + file I/O). No signature crosses the bridge except inside an exported link
code, which is a public attestation by design.

## Proof

* `src-tauri/src/device_link_tests.rs`: the golden message vectors shared byte-for-byte with
  cluster-core; the device key mint/reuse/fail-closed rules; EIP-191 sign + recover (low-s, v 27/28);
  the store; the mesh-identity choice (unchanged by default); the full flow over a real vault and a
  real ceremony, where the wallet signature recovers to the wallet and a spent or declined ceremony
  cannot produce another link. Mutation-checked.
* `src-tauri/src/cluster_tests.rs`: the SetRoster wire stays byte-identical for an older daemon and
  carries links in the daemon's shape when present.
* `src/shell/deviceLink.store.test.ts`: the review path signs nothing on open, approves through the
  dedicated command (never `signing.broadcast`), and declines through its own reject.
* citrate-cluster: TLC on `DeviceLink.tla` and a three-process loopback test with real keys.
* Owed: the two-machine run on separate hosts (DGX), and the CL-S4 transport sign-off.
