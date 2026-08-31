// CONNECT-S3 — Connections social-identity repositioning honesty.
//
// The social flow is WIRED (bridge.social → src-tauri/src/social.rs: OAuth ownership proof, keyring
// token, wallet-signed IdentityBinding via the ceremony, resolve/export/ingest). It had shipped
// behind a false "pending backend" flag with a "NOT WIRED" header — the dead-end framing the connect
// realignment (docs/CONNECT_REALIGN_PLANSET.md, D-4) exists to kill. S3 repositions it to its real
// job and forbids the one thing it must never claim.
//
// These render Connections and assert, at the static-markup level:
//   1. The section states the REAL payoff — face + invite-by-@handle.
//   2. The explicit no-import disclaimer is present (Rule 1: X/Discord don't expose followers).
//   3. The false "pending backend" flag was removed from the social section (only the genuinely
//      not-wired SaaS + Webhooks sections keep it → exactly 2 occurrences remain, down from 3).
import { describe, it, expect } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";

import { Connections } from "./Connections";
import { freshState } from "../shell/state";
import type { Store } from "../shell/store";

// Connections touches `store` only in effects/handlers (never fired by a static render); a bare stub
// covers the render. Initial state → links/mcp are empty, so the unlinked rows + section copy render.
const noopStore = {} as unknown as Store;
const render = () => renderToStaticMarkup(<Connections store={noopStore} s={freshState("p1")} />);

describe("Connections — CONNECT-S3 social repositioning honesty", () => {
  it("states the real payoff: a face + invite-by-@handle, not a dead-end", () => {
    const html = render().toLowerCase();
    expect(html).toContain("invite you by @handle");
    expect(html).toContain("becomes your face");
  });

  it("carries the explicit no-import disclaimer (never claims a friend/follower import)", () => {
    const html = render().toLowerCase();
    // POSITIVE — the disclaimer is present and specific.
    expect(html).toContain("does");
    expect(html).toContain("import your followers, friends, or contacts");
    // NEGATIVE CONTROL — no affirmative import promise anywhere.
    expect(html).not.toContain("import your friends");
    expect(html).not.toContain("import your contacts");
    expect(html).not.toContain("find your followers");
  });

  it("says a Groups-visible handle becomes your face in the directory + picker", () => {
    const html = render().toLowerCase();
    expect(html).toContain("face in the people directory");
    expect(html).toContain("add-member picker");
  });

  it("removes the false 'pending backend' flag from the wired social section", () => {
    const html = render();
    // The flag legitimately remains on SaaS tools + Webhooks (still not wired) → exactly 2, down from
    // 3 when Social identity wrongly carried it too.
    const occurrences = html.split("pending backend").length - 1;
    expect(occurrences).toBe(2);
  });
});
