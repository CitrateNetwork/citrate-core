// 0.5.4: the forced update. Only a minimum Rust verified (update_policy.rs) gates the app; the
// feed's own fields are never trusted here, a gate is never lifted by a later verdict this session,
// and the gated card cannot be dismissed.
import { describe, it, expect, vi } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { renderToStaticMarkup } from "react-dom/server";
import { INITIAL_UPDATE_STATE, REQUIRED_RECHECK_MS, feedPolicy, withVerdict, type PolicyVerdict, type UpdateState } from "./updater";
import { MANUAL_DOWNLOAD_URL, SUPPORT_EMAIL, UpdateCard, UpdateRequiredHelp, UpdateRequiredScreen } from "./UpdateBanner";
import { createRoot } from "react-dom/client";
import { act } from "react";

const verdict = (over: Partial<PolicyVerdict> = {}): PolicyVerdict => ({
  currentVersion: "0.5.3",
  minimumVersion: "0.5.4",
  updateRequired: true,
  reason: "0.5.4 fixes Hermes",
  refused: null,
  ...over,
});
const noop = () => undefined;

describe("feedPolicy reads only the signed pair, as strings", () => {
  it("takes latest.json citrate.updatePolicy", () => {
    expect(feedPolicy({ version: "0.5.4", citrate: { updatePolicy: { policy: "cA==", signature: "c2ln" } } })).toEqual({ policy: "cA==", signature: "c2ln" });
  });
  it("an unsigned minimumVersion anywhere in the feed is not a policy", () => {
    expect(feedPolicy({ version: "0.5.4", minimumVersion: "0.5.4" })).toBeNull();
    expect(feedPolicy({ citrate: { minimumVersion: "0.5.4" } })).toBeNull();
    expect(feedPolicy({ citrate: { updatePolicy: { policy: "cA==" } } })).toBeNull();
    expect(feedPolicy({ citrate: { updatePolicy: { policy: 1, signature: 2 } } })).toBeNull();
    expect(feedPolicy(null)).toBeNull();
    expect(feedPolicy("latest")).toBeNull();
  });
});

describe("withVerdict", () => {
  it("a verified minimum above this version gates", () => {
    const s = withVerdict(INITIAL_UPDATE_STATE, verdict());
    expect(s).toMatchObject({ required: true, minimumVersion: "0.5.4", requiredReason: "0.5.4 fixes Hermes", dismissed: false });
  });
  it("not required (or refused with nothing kept) never gates", () => {
    expect(withVerdict(INITIAL_UPDATE_STATE, verdict({ updateRequired: false, minimumVersion: null, refused: "bad signature" })).required).toBe(false);
  });
  it("a refused replay keeps the gate (Rust answers with the kept policy's verdict)", () => {
    const gated = withVerdict(INITIAL_UPDATE_STATE, verdict());
    const replay = withVerdict(gated, verdict({ refused: "the feed's update policy was not used: it was issued at 10, not after the kept policy (20)" }));
    expect(replay).toMatchObject({ required: true, minimumVersion: "0.5.4" });
  });
  it("a later signed policy that lowers or rescinds the minimum lifts the gate", () => {
    const gated = withVerdict(INITIAL_UPDATE_STATE, verdict());
    expect(withVerdict(gated, verdict({ updateRequired: false, minimumVersion: "0.5.2", reason: null }))).toMatchObject({ required: false, minimumVersion: "0.5.2", requiredReason: null });
    expect(withVerdict(gated, verdict({ updateRequired: false, minimumVersion: null, reason: null })).required).toBe(false);
  });
  it("a gate un-dismisses the card", () => {
    expect(withVerdict({ ...INITIAL_UPDATE_STATE, dismissed: true }, verdict()).dismissed).toBe(false);
  });
});

describe("the gate on screen", () => {
  const gated = (over: Partial<UpdateState> = {}): UpdateState => ({ ...withVerdict(INITIAL_UPDATE_STATE, verdict()), ...over });

  it("nothing shows without a verified gate", () => {
    expect(renderToStaticMarkup(<UpdateRequiredScreen state={INITIAL_UPDATE_STATE} onRetry={noop} />)).toBe("");
  });

  it("the blocking screen names the minimum and the reason", () => {
    const html = renderToStaticMarkup(<UpdateRequiredScreen state={gated({ status: "downloading" })} onRetry={noop} />);
    expect(html).toContain('role="dialog"');
    expect(html).toContain('aria-modal="true"');
    expect(html).toContain("Citrate Core 0.5.4 or later");
    expect(html).toContain("0.5.4 fixes Hermes");
    expect(html).toContain("The update is downloading.");
  });

  it("with no update in hand it says so and offers a retry", () => {
    const html = renderToStaticMarkup(<UpdateRequiredScreen state={gated({ status: "uptodate" })} onRetry={noop} />);
    expect(html).toContain("No update could be found right now");
    expect(html).toContain("Check for the update");
  });

  it("the gated card has no Dismiss or Later, only the way forward", () => {
    const ready = renderToStaticMarkup(<UpdateCard state={gated({ status: "ready", version: "0.5.4" })} install={noop} restart={noop} dismiss={noop} />);
    expect(ready).toContain("Restart now");
    expect(ready).not.toContain("Later");
    const avail = renderToStaticMarkup(<UpdateCard state={gated({ status: "available", version: "0.5.4" })} install={noop} restart={noop} dismiss={noop} />);
    expect(avail).toContain("Required update");
    expect(avail).not.toContain("Dismiss");
    // An ungated card keeps its Dismiss.
    const plain = renderToStaticMarkup(<UpdateCard state={{ ...INITIAL_UPDATE_STATE, status: "available", version: "0.5.4" }} install={noop} restart={noop} dismiss={noop} />);
    expect(plain).toContain("Dismiss");
  });
});

describe("wiring", () => {
  const src = readFileSync(resolve(__dirname, "updater.ts"), "utf8");
  it("the feed's policy goes to Rust with the feed's own version, and the kept one is read at launch", () => {
    expect(src).toMatch(/invoke<PolicyVerdict>\("update_policy_apply", \{ policy: policy\.policy, signature: policy\.signature, feedVersion: update\.version \}\)/);
    expect(src).toMatch(/invoke<PolicyVerdict>\("update_policy_status"/);
  });
  it("gated with no update offered, Rust reads the feed itself (a rescind after a pulled release)", () => {
    expect(src).toMatch(/if \(gatedRef\.current\) \{[\s\S]*invoke<PolicyVerdict>\("update_policy_refresh"/);
  });
  it("a required update downloads by itself; the restart stays a click", () => {
    expect(src).toMatch(/state\.status === "available" && \(state\.critical \|\| state\.required\)\) void install\(\)/);
    expect(src).not.toMatch(/downloadAndInstall/);
  });
});

describe("the way out while gated (#678 review)", () => {
  const gated = (over: Partial<UpdateState> = {}): UpdateState => ({ ...withVerdict(INITIAL_UPDATE_STATE, verdict()), ...over });

  it("the block screen offers Quit, a manual download, and support, in every state", () => {
    for (const status of ["available", "downloading", "ready", "error", "uptodate"] as const) {
      const html = renderToStaticMarkup(<UpdateRequiredScreen state={gated({ status })} onRetry={noop} />);
      expect(html, status).toContain("Quit Citrate Core");
      expect(html, status).toContain("Download manually");
      expect(html, status).toContain(`href="${MANUAL_DOWNLOAD_URL}"`);
      expect(html, status).toContain("ask support for a fresh one");
      expect(html, status).toContain(SUPPORT_EMAIL);
    }
  });

  it("points at the download page, never an installer URL", () => {
    expect(MANUAL_DOWNLOAD_URL).toBe("https://www.citrate.ai/download");
    const html = renderToStaticMarkup(<UpdateRequiredHelp error={null} />);
    expect(html).not.toMatch(/\.(dmg|AppImage|deb|msi|exe|tar\.gz)/);
    expect(SUPPORT_EMAIL).toBe("support@citrate.ai");
    // The address is selectable text.
    expect(html).toMatch(/user-select:all[^>]*>support@citrate\.ai</);
  });

  it("shows the update error when there was one, and nothing when there was not", () => {
    const withErr = renderToStaticMarkup(<UpdateRequiredScreen state={gated({ status: "error", error: "signature verification failed" })} onRetry={noop} />);
    expect(withErr).toContain("What went wrong");
    expect(withErr).toContain("signature verification failed");
    expect(renderToStaticMarkup(<UpdateRequiredScreen state={gated({ status: "downloading" })} onRetry={noop} />)).not.toContain("What went wrong");
  });

  it("Quit and Copy do what they say", async () => {
    const quit = vi.fn();
    const copy = vi.fn(async () => true);
    const el = document.createElement("div");
    document.body.appendChild(el);
    const root = createRoot(el);
    await act(async () => root.render(<UpdateRequiredHelp error={null} onQuit={quit} onCopy={copy} />));
    const button = (label: string) => [...el.querySelectorAll("button")].find((b) => b.textContent === label)!;
    await act(async () => button("Quit Citrate Core").click());
    expect(quit).toHaveBeenCalledTimes(1);
    await act(async () => button("Copy").click());
    expect(copy).toHaveBeenCalledWith("support@citrate.ai");
    expect(button("Copied")).toBeTruthy();
    // A clipboard that refuses says so.
    copy.mockImplementationOnce(async () => false);
    await act(async () => button("Copied").click());
    expect(el.textContent).toContain("select the address instead");
    await act(async () => root.unmount());
    el.remove();
  });

  it("nothing loops while gated: one re-check every few minutes, the auto-download only from 'available'", () => {
    const src = readFileSync(resolve(__dirname, "updater.ts"), "utf8");
    expect(REQUIRED_RECHECK_MS).toBeGreaterThanOrEqual(60_000);
    expect(src).toMatch(/window\.setTimeout\(\(\) => void check\(\), REQUIRED_RECHECK_MS\)/);
    expect(src).toMatch(/return \(\) => window\.clearTimeout\(id\)/);
    // An error never restarts the download by itself (it waits for the next re-check or Retry).
    expect(src).toMatch(/state\.status === "available" && \(state\.critical \|\| state\.required\)\) void install\(\)/);
    expect((src.match(/setInterval/g) ?? []).length).toBe(1); // the 6 h check only
  });
});
