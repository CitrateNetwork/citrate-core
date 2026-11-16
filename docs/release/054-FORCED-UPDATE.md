---
created: 2026-11-14
branch: fix/054-forced-update (release steps: docs/054-forced-update-release-steps)
author: Larry Klosowski (Claude Opus 5.5, 0.5.4 Hermes lane)
status: active (implemented; owner sign-off 2026-11-14: live in 0.5.4, updater key signs the policy)
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

**Support: check the device clock.** The same rule cuts the other way. A device whose clock is more
than 24 h *behind* sees every new policy as dated too far ahead, so it refuses them all, including
a policy that lowers or rescinds the minimum. Such a member stays gated by the policy already kept
on the device after everyone else is released. When a gated member reports that a lowered or
rescinded minimum did not reach them, first ask them to set the device clock to automatic, then
check for the update again (the refusal is logged as "dated ... ahead of this device's clock").

Below the minimum, the app shows a blocking screen (`src/shell/UpdateBanner.tsx`). The update downloads by itself, the card cannot be dismissed, and Restart now is the way forward.

For members whose updater cannot install, the screen also offers a manual way out:

- **Quit Citrate Core.**
- **Download manually:** a link to https://www.citrate.ai/download. Installers are behind the invite gate, so the screen says support can send a fresh invite link. No installer URL is shown.
- **Support:** support@citrate.ai as selectable text, with a Copy button.
- **What went wrong:** the update error text, when there is one.

While gated, the app itself is not rendered at all: the window holds only the update dialog
(focus trapped inside it, the update card drawn within it), so no wallet, send or sign control
exists to focus or click, and app keyboard shortcuts are blocked. The wallet is not reachable,
read-only or otherwise. Links open in the system browser through core's `open_external`. Nothing on the screen loops: it checks again once every 5 minutes (one timer), and the automatic download starts only when an update is offered, never on its own after an error. Anything unverified gates nothing: it fails open, and the reason is logged.

## Release steps: 0.5.4 (the release lane runs these)

Owner 2026-11-14: "I also need the forced update to be live as well in this release." This is the
sign-off on signing the policy with the updater key. `scripts/release-local.sh --policy-min` does
the whole policy step inside the release, on the release Mac:

- **When it's written:** just before the signing step, outside it, so the node script that writes it never has the key in its environment (`env -u` removes the key variables even if the caller's shell had them). Its `issuedAt` is the release time, the same instant as the feed's `pub_date`.
- **How it's signed:** with `tauri signer sign`, in the same subshell and with the same key handling as the update files:
  - The key comes from `UPDATER_KEY_FILE` (default `~/.citrate-updater/citrate-core-v2.key`).
  - The passphrase comes from the macOS Keychain item `citrate-core-updater-v2`.
  - Both are set only in that subshell's environment, never on argv.
- **How it's checked:** with `minisign -V` against the pinned key, then embedded in `latest.json` as `citrate.updatePolicy`. The script then checks that the feed's policy is byte for byte the signed file, with minimum and `issuedAt` as given.
- **What it refuses:** a minimum above `--version`, and notes without `[critical]`.

Commands, from the 0.5.4 release commit on the release Mac:

```bash
cd citrate-core
export RELEASE_NOTES="[critical] Citrate Core 0.5.4: Hermes starts and reports its setup correctly, and from now on the app can require an update. Restart to install."

# 1. Check the arguments (no signing, nothing published).
scripts/release-local.sh --check-args --version 0.5.4 --linux-arches x86_64 --publish-target spaces \
  --policy-min 0.5.4 --linux-dir <linux update files> --out <out dir>
#    expect: "update policy: minimum 0.5.4, signed with the updater key at release time, embedded in latest.json"

# 2. The release, as 0.5.3 was released (053-UPDATER.md), plus --policy-min.
scripts/release-local.sh --version 0.5.4 --linux-arches x86_64 --publish-target spaces \
  --policy-min 0.5.4 --linux-dir <linux update files> --out <out dir> --publish
#    expect: "latest.json carries the signed update policy: minimum 0.5.4 issuedAt <release time>"

# 3. After publishing, check the live feed carries the policy.
curl -s https://citrate-cdn.nyc3.cdn.digitaloceanspaces.com/downloads/updater/latest.json \
  | python3 -c "import json,sys,base64;f=json.load(sys.stdin);print(f['version'],'[critical]' in f['notes'],json.loads(base64.b64decode(f['citrate']['updatePolicy']['policy'])))"
#    expect: 0.5.4 True {'domain': 'citrate-core-update-policy/v1', 'minimumVersion': '0.5.4', ...}
```

What this does, honestly:

- **0.5.3 and older can't be forced.** They ignore the policy. `[critical]` in the notes makes them download in the background and show a red card, but each member still has to click Restart.
- **Nobody is gated by this policy today.** A minimum of 0.5.4 gates only apps older than 0.5.4 that read the policy, and the first app that reads it is 0.5.4 itself. The exception is a 0.5.4 pre-release build (`0.5.4-rc.N`), which counts as older than 0.5.4 and is gated.
- **What it does do:** it puts a verified, signed minimum on every 0.5.4 install, which proves the path end to end. A later release that raises the minimum (for example `--policy-min 0.5.5`) is what gates 0.5.4 installs.
- **Feed fallback:** 0.5.3's GitHub feed fallback, and a feed without a policy, are unchanged.

Manual form (only if `release-local.sh` cannot be used; same key handling, never argv):

1. `node scripts/updater-feed.mjs policy --min 0.5.4 --reason "Citrate Core 0.5.4 or later is required" --issued-at "$(date -u +%s)" --out update-policy.json`. This refuses an `issuedAt` more than 24 h ahead.
2. Sign it with `TAURI_SIGNING_PRIVATE_KEY="$(cat "$UPDATER_KEY_FILE")"` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD="$(security find-generic-password -s citrate-core-updater-v2 -a citrate-updater -w)"` exported in a subshell, then run `tauri signer sign update-policy.json`. Check it: `base64 -d < update-policy.json.sig > p.minisig && minisign -V -P <pinned key> -m update-policy.json -x p.minisig`.
3. `node scripts/updater-feed.mjs merge --notes "$RELEASE_NOTES" --pub-date <RFC 3339 UTC> --policy update-policy.json <fragments>`. This refuses a policy above the feed's version or one without a signature.
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

For a lower or rescind the manual form above is the way (`release-local.sh --policy-min` only
writes a minimum for the release it builds).

## Sign-off (@rule8)

- **Reusing the updater key for the policy: signed off by the owner, 2026-11-14** ("I also need the forced update to be live as well in this release"). It adds no power: the updater key already decides which binary every member runs. The domain inside the signed bytes keeps a policy from passing as an artifact, and an artifact from passing as a policy.
- **Who may raise the minimum, and when.** A raised minimum blocks every older client until it updates. Raise it only when the feed's version is published on both endpoints for every platform.
