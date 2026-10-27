// core#423 — owner decision 2026-10-27: v2 invite links are hidden in 0.5.2 release builds, and a
// release build can't turn them on while kit/src/join.rs pins no key. Written red-first against
// 155f0943.
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { renderToStaticMarkup } from "react-dom/server";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { joinPinnedKeyCount, v2LinksBuildRefusal } from "./v2LinksBuildGuard";
import { v2InviteLinksEnabled, V2_LINKS_NOT_AVAILABLE, V2_LINK_NOT_ACCEPTED } from "./v2Links";
import { InviteLinks, YourInviteLinks } from "../surfaces/InviteLinks";
import { Community } from "../surfaces/Community";
import { store } from "../shell/store";
import { bridge } from "../bridge";
import { freshState } from "../shell/state";
import type { Store } from "../shell/store";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const ROOT = join(__dirname, "..", "..");
const JOIN_RS = readFileSync(join(ROOT, "kit/src/join.rs"), "utf8");
const EMPTY = "pub const JOIN_PINNED_KEYS: &[(&str, &str)] = &[];";
const ONE = 'pub const JOIN_PINNED_KEYS: &[(&str, &str)] = &[\n    ("join-1", "' + "A".repeat(43) + '"),\n];';
const TWO = 'pub const JOIN_PINNED_KEYS: &[(&str, &str)] = &[("join-1", "' + "A".repeat(43) + '"), ("join-2", "' + "B".repeat(43) + '")];';

describe("the release-build guard", () => {
  it("counts the pinned keys", () => {
    expect(joinPinnedKeyCount(EMPTY)).toBe(0);
    expect(joinPinnedKeyCount(ONE)).toBe(1);
    expect(joinPinnedKeyCount(TWO)).toBe(2);
    expect(joinPinnedKeyCount("no such constant")).toBeNull();
  });

  it("reads the real kit/src/join.rs", () => {
    expect(joinPinnedKeyCount(JOIN_RS)).not.toBeNull();
  });

  it("refuses a build that turns v2 links on with no pinned key", () => {
    const why = v2LinksBuildRefusal("1", EMPTY);
    expect(why).not.toBeNull();
    expect(why ?? "").toMatch(/JOIN_PINNED_KEYS/);
    expect(why ?? "").toMatch(/core#423/);
  });

  it("refuses when the pins can't be read (fails closed)", () => {
    expect(v2LinksBuildRefusal("1", "garbled")).not.toBeNull();
  });

  it("lets a build with v2 links off, or with a pinned key, go on", () => {
    expect(v2LinksBuildRefusal(undefined, EMPTY)).toBeNull();
    expect(v2LinksBuildRefusal("0", EMPTY)).toBeNull();
    expect(v2LinksBuildRefusal("1", ONE)).toBeNull();
  });

  it("vite.config.ts runs the guard on every production build", () => {
    const cfg = readFileSync(join(ROOT, "vite.config.ts"), "utf8");
    expect(cfg).toContain("v2LinksBuildRefusal(");
    expect(cfg).toMatch(/command === "build"/);
  });

  it("no workflow turns v2 links on while no key is pinned", () => {
    if (joinPinnedKeyCount(JOIN_RS) !== 0) return;
    const dir = join(ROOT, ".github/workflows");
    for (const f of ["ci.yml", "release.yml"]) {
      expect(readFileSync(join(dir, f), "utf8"), f).not.toMatch(/VITE_V2_INVITE_LINKS/);
    }
  });
});

describe("v2 links are off unless the build turns them on", () => {
  afterEach(() => vi.unstubAllEnvs());

  it("off by default", () => {
    vi.stubEnv("VITE_V2_INVITE_LINKS", "");
    expect(v2InviteLinksEnabled()).toBe(false);
  });

  it("on only for exactly 1", () => {
    vi.stubEnv("VITE_V2_INVITE_LINKS", "1");
    expect(v2InviteLinksEnabled()).toBe(true);
    vi.stubEnv("VITE_V2_INVITE_LINKS", "true");
    expect(v2InviteLinksEnabled()).toBe(false);
  });
});

/** Static markup with the apostrophe React escapes put back. */
const text = (html: string) => html.replace(/&#x27;/g, "'");

describe("with v2 links off, no surface offers one (D-6: an honest not-available)", () => {
  const toastStore = { toast: () => {} } as unknown as Store;
  beforeEach(() => vi.stubEnv("VITE_V2_INVITE_LINKS", ""));
  afterEach(() => vi.unstubAllEnvs());

  it("Groups › invite links shows not-available, with no create control", () => {
    const html = text(renderToStaticMarkup(<InviteLinks store={toastStore} groupId="grp_1" groupName="Garden" />));
    expect(html).toContain(V2_LINKS_NOT_AVAILABLE);
    expect(html).not.toMatch(/<button/);
  });

  it("Your invite links never asks the service for codes", async () => {
    const list = vi.spyOn(bridge.join, "list").mockResolvedValue([]);
    const host = document.createElement("div");
    const root = createRoot(host);
    await act(async () => root.render(<YourInviteLinks store={toastStore} />));
    expect(list).not.toHaveBeenCalled();
    expect(host.innerHTML).not.toMatch(/<button/);
    act(() => root.unmount());
    list.mockRestore();
  });

  it("Community offers no public link", () => {
    const html = text(renderToStaticMarkup(<Community store={toastStore} s={freshState("p1")} />));
    expect(html).not.toContain("Create a public link");
    expect(html).toContain(V2_LINKS_NOT_AVAILABLE);
  });
});

describe("with v2 links off, an arriving v2 link is not held", () => {
  const LINK = "https://citrate.ai/join/Ab3dE7gh#" + "A".repeat(22);
  let set: ReturnType<typeof vi.spyOn>;
  let toast: ReturnType<typeof vi.spyOn>;
  beforeEach(() => {
    vi.stubEnv("VITE_V2_INVITE_LINKS", "");
    store.setState({ pendingJoin: null, pendingInvite: null, stage: "done", signedIn: true });
    set = vi.spyOn(bridge.join, "pendingSet").mockResolvedValue({ hasKey: true, storedAt: 1, expiresAt: 2, persisted: true });
    toast = vi.spyOn(store, "toast").mockImplementation(() => {});
  });
  afterEach(() => {
    vi.unstubAllEnvs();
    vi.restoreAllMocks();
  });

  it("a deep link is refused with a reason and never reaches Rust", async () => {
    store.handleDeepLink(LINK);
    await new Promise((r) => setTimeout(r, 0));
    expect(set).not.toHaveBeenCalled();
    expect(store.state.pendingJoin).toBeNull();
    expect(toast).toHaveBeenCalledWith(V2_LINK_NOT_ACCEPTED);
  });

  it("a pasted link is consumed with the reason, not held", async () => {
    expect(await store.acceptInviteInput(LINK)).toBe(true);
    expect(set).not.toHaveBeenCalled();
    expect(toast).toHaveBeenCalledWith(V2_LINK_NOT_ACCEPTED);
  });
});
