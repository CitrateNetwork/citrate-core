---
created: 2026-11-14
branch: fix/054-forced-update
author: Larry Klosowski (Claude Opus 5.5, 0.5.4 Hermes lane)
status: active (implemented; the signing step and key reuse need security sign-off, @rule8)
---

# 0.5.4: forced update to a minimum version

Owner request (2026-11-14): clients below a minimum version must update before they can use the app.

## What 0.5.3 clients will do

0.5.3 has no version gate. It ignores any field in `latest.json` that it doesn't know, so a
`minimumVersion` or `citrate.updatePolicy` is skipped.

What 0.5.3 does with a newer feed (`src/shell/updater.ts` as released in 0.5.3):

- It checks at launch and every 6 h, then shows the bottom-right update card. The card can be dismissed, and it comes back at the next check.
- If the feed's `notes` contain `[critical]` (or start with `critical:`), 0.5.3 downloads the update in the background and shows a red "Critical update available" card.
  - This is the strongest push 0.5.3 has, so **put `[critical]` in the 0.5.4 notes**.
  - Installing still needs the member to click Restart now.
  - "Later" hides the card until the next check, and the plugin never installs on its own.
  - `notes` are not signed. A feed tamperer could add or remove the marker, which changes the download prompt and nothing else.

So no 0.5.3 client can be forced. Each one has to click Restart once. From 0.5.4 on, the gate below applies.

## What 0.5.4 enforces

`latest.json` is not signed. The updater plugin verifies only each downloaded artifact. A minimum
read from the feed as is would let anyone who can change the feed lock every member out. So the
minimum is a separate document, signed with the updater key:

```json
"citrate": { "updatePolicy": { "policy": "<base64 of the policy bytes>", "signature": "<base64 of the .sig text>" } }
```

The policy bytes are
`{"domain":"citrate-core-update-policy/v1","minimumVersion":"0.5.4","issuedAt":<unix s>,"reason":"..."}`.
`"minimumVersion": null` means no minimum (a rescind).

`src-tauri/src/update_policy.rs` verifies the policy:

- It checks the signature exactly as the plugin checks an artifact: the same pinned key from `tauri.conf.json`, the same minisign format.
- It checks the domain, and refuses unknown fields.
- It refuses a minimum above the version the same feed offers, because the app would be gated with nothing to update to.

The verified policy is kept in `<app data>/update-policy.json` and verified again at every read, so
launching with no network does not lift a gate the app has already seen.

Policies are ordered by their signed `issuedAt` alone. A verified policy replaces the kept one only
when its `issuedAt` is **strictly later**, whatever its minimum: it may raise, lower or rescind it.
A replayed older policy, or a different policy with the same `issuedAt`, is refused and the kept one
stays (the refusal is logged).
A policy whose `issuedAt` is more than 24 h ahead of the client's clock is refused too, and
`updater-feed.mjs policy` refuses to write one. Otherwise a mis-set signing clock would date a
policy that outranks every correction signed before that date.

Below the minimum, the app shows a blocking screen (`src/shell/UpdateBanner.tsx`). The update downloads by itself, the card cannot be dismissed, and Restart now is the only way forward. Anything unverified gates nothing: it fails open, and the reason is logged.

## Release steps (once per release that raises the minimum)

1. `node scripts/updater-feed.mjs policy --min 0.5.4 --reason "0.5.4 fixes Hermes" --issued-at $(date +%s) --out update-policy.json`
2. `tauri signer sign update-policy.json` with the updater key (the same step as the artifacts in `scripts/release-local.sh`). Check it with `minisign -V -p <pub> -m update-policy.json -x update-policy.json.sig`, after decoding the base64 `.sig` into a file.
3. `node scripts/updater-feed.mjs merge --notes "[critical] ..." --pub-date ... --policy update-policy.json <fragments>`. This refuses a policy above the feed's version or one without a signature.
4. Upload `latest.json` to both endpoints as usual (`053-UPDATER.md`).

## Lowering or rescinding a minimum

If a minimum was signed by mistake, or the release it points at is pulled, clients stay gated with
nothing to install until a newer policy reaches them. Fix it the same way, with a later `issuedAt`:

1. Lower: `node scripts/updater-feed.mjs policy --min <the version members can install> --reason "..." --issued-at $(date +%s) --out update-policy.json`.
   Rescind entirely: use `--min none` instead (writes `"minimumVersion": null`).
2. Sign it with the updater key and check it, as in step 2 above.
3. `merge --policy update-policy.json` into the feed of the version that is actually published, and
   upload `latest.json` to both endpoints. A rescind is accepted with any feed version. If the feed
   now offers no version newer than the gated clients run (the release was pulled), the updater
   plugin reports "no update"; a gated client then reads the feed itself (`update_policy_refresh`)
   so it still gets the new policy.

The new `issuedAt` must be later than the one being replaced (`date +%s` at signing time is). Gated
clients keep checking every 5 minutes, so they pick it up without a restart. Never publish the
mistaken policy's feed again afterwards: it is refused by clients that saw the newer one, but a
fresh install that sees only the old feed would be gated.

`scripts/release-local.sh` does not run steps 1 and 2 yet. They are manual until security signs off.

## Sign-off needed (@rule8)

- **Reusing the updater key for the policy.** It adds no power: the updater key already decides which binary every member runs. The domain inside the signed bytes keeps a policy from passing as an artifact, and an artifact from passing as a policy.
- **Who may raise the minimum, and when.** A raised minimum blocks every older client until it updates. Raise it only when the feed's version is published on both endpoints for every platform.
