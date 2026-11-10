import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { AccessStoreError, accessAllowsSetup, gateScreen } from "./AccessPassGate";
import { ACCESS_OFF, accessPassStatus, finishedOnboardingBefore } from "./accessPass";

// What 0.5.0 to 0.5.2 persist under `citrate-core-proto-v2` once onboarding is finished (the
// `stage: D` key of PERSIST_SCOPE, plus the genesis stamp). Nothing else those builds write marks a
// finished onboarding: config.json is saved on every quit, custody.enc is made on every launch.
const SAVE_052_DONE = JSON.stringify({
  persona: "p1",
  tier: "free",
  org: "",
  citrateRole: "",
  entitlement: "public",
  stage: "done",
  s2: "done",
  s3: "free",
  hasGrant: false,
  netGenesis: "0xabc",
});

describe("first-run access check (download gate)", () => {
  it("lets setup through only when the check says nothing is required", () => {
    expect(accessAllowsSetup({ kind: "ready", view: ACCESS_OFF })).toBe(true);
    expect(accessAllowsSetup({ kind: "ready", view: { gate: "on", required: false, recorded: "existing", platform: null } })).toBe(true);
    expect(accessAllowsSetup({ kind: "ready", view: { gate: "on", required: true, recorded: null, platform: null } })).toBe(false);
    expect(accessAllowsSetup({ kind: "loading" })).toBe(false);
    expect(accessAllowsSetup({ kind: "error", message: "x" })).toBe(false);
  });

  it("a store error is shown as a local storage problem, never as the code prompt", () => {
    expect(gateScreen({ kind: "loading" })).toBe("wait");
    expect(gateScreen({ kind: "ready", view: ACCESS_OFF })).toBe("setup");
    expect(gateScreen({ kind: "ready", view: { gate: "on", required: true, recorded: null, platform: null } })).toBe("code");
    expect(gateScreen({ kind: "error", message: "Could not save the access check on this computer: pass.json is damaged" })).toBe("store-error");
    const html = renderToStaticMarkup(<AccessStoreError message="Could not save the access check on this computer: disk full" onRetry={() => {}} />);
    expect(html).toContain("disk full");
    expect(html).toContain("access-pass-store-error");
    expect(html).not.toContain("access-pass-input");
    expect(html).not.toMatch(/access code/i);
  });

  it("only a finished onboarding (persisted stage done) marks an earlier install", () => {
    expect(finishedOnboardingBefore(SAVE_052_DONE)).toBe(true);
    // A fresh install that quit at the gate: nothing saved, or a save still at the welcome step.
    expect(finishedOnboardingBefore(null)).toBe(false);
    expect(finishedOnboardingBefore("")).toBe(false);
    expect(finishedOnboardingBefore(JSON.stringify({ ...JSON.parse(SAVE_052_DONE), stage: "s0" }))).toBe(false);
    expect(finishedOnboardingBefore(JSON.stringify({ ...JSON.parse(SAVE_052_DONE), stage: "s3" }))).toBe(false);
    // Damaged or foreign saves are not read as finished.
    for (const bad of ["garbage", "[]", "null", '"done"', '{"stage":true}', '[{"stage":"done"}]']) {
      expect(finishedOnboardingBefore(bad)).toBe(false);
    }
  });

  it("reads as off in the web preview (no gate, nothing asked)", async () => {
    await expect(accessPassStatus()).resolves.toEqual(ACCESS_OFF);
  });

  it("the panel copy names no price, token or reward", async () => {
    const { AccessPassGate } = await import("./AccessPassGate");
    // Static render shows the loading state (null); the copy lives in the module source.
    expect(renderToStaticMarkup(<AccessPassGate store={{} as never} s={{} as never}>x</AccessPassGate>)).toBe("");
    const { readFileSync } = await import("node:fs");
    const { resolve } = await import("node:path");
    const src = readFileSync(resolve(process.cwd(), "src/onboarding/AccessPassGate.tsx"), "utf8");
    for (const banned of [/\bSALT\b/, /reward/i, /\$\d/, /—/]) expect(src).not.toMatch(banned);
  });
});
