// CX-S3.4 — Groups surface copy + honesty tripwires (Rule 1 / RT-6 reach scoping / copy-lint).
//
// The chat surface must (1) scope reach to "your Groups", never blanket message-anyone reach (RT-6:
// the web<->native bridge isn't delivered), (2) render honest EMPTY states with no fabricated room,
// roster, or message on a fresh mount (effects don't fire in static markup, so this is the pre-load
// state), and (3) be truthful that RBAC is enforced at the relay, not the client. Red-then-green:
// these fail if the forbidden phrasing renders or a fake conversation is drawn.
import { describe, it, expect } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { Groups } from "./Groups";
import { freshState } from "../shell/state";
import type { Store } from "../shell/store";

const stubStore = { toast: () => {} } as unknown as Store;
const html = renderToStaticMarkup(<Groups store={stubStore} s={freshState("p1")} />);

describe("Groups honesty — scoped reach, honest-empty, relay-enforced RBAC", () => {
  it("scopes reach to your Groups, never claiming blanket message-anyone reach (RT-6)", () => {
    // NB: \s+ (not " +") so this assertion's own source doesn't trip the space-based copy-lint.
    expect(html).not.toMatch(/message\s+anyone\s+on\s+citrate/i);
    expect(html.toLowerCase()).toContain("in your groups");
  });

  it("renders honest empty states with no fabricated room or message (Rule 1)", () => {
    // Fresh mount, before any bridge load: the group list + conversation are empty prompts.
    expect(html.toLowerCase()).toContain("no groups yet");
    expect(html.toLowerCase()).toContain("select a group");
  });

  it("is truthful that roles are enforced at the relay, not the client", () => {
    expect(html.toLowerCase()).toContain("enforced at the relay");
  });

  it("promises no guaranteed privacy/unbreakability — states the concrete property (ciphertext-only relay)", () => {
    expect(html).not.toMatch(/unbreakable|guaranteed +(privacy|security)/i);
    expect(html.toLowerCase()).toContain("ciphertext");
  });
});
