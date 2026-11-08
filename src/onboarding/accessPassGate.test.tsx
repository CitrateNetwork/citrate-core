import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { accessAllowsSetup } from "./AccessPassGate";
import { ACCESS_OFF, accessPassStatus } from "./accessPass";

describe("first-run access check (download gate)", () => {
  it("lets setup through only when the check says nothing is required", () => {
    expect(accessAllowsSetup({ kind: "ready", view: ACCESS_OFF })).toBe(true);
    expect(accessAllowsSetup({ kind: "ready", view: { gate: "on", required: false, recorded: "existing", platform: null } })).toBe(true);
    expect(accessAllowsSetup({ kind: "ready", view: { gate: "on", required: true, recorded: null, platform: null } })).toBe(false);
    expect(accessAllowsSetup({ kind: "loading" })).toBe(false);
    expect(accessAllowsSetup({ kind: "error", message: "x" })).toBe(false);
  });

  it("reads as off in the web preview (no gate, nothing asked)", async () => {
    await expect(accessPassStatus()).resolves.toEqual(ACCESS_OFF);
  });

  it("the panel copy names no price, token or reward", async () => {
    const { AccessPassGate } = await import("./AccessPassGate");
    // Static render shows the loading state (null); the copy lives in the module source.
    expect(renderToStaticMarkup(<AccessPassGate store={{} as never} s={{} as never}>x</AccessPassGate>)).toBe("");
    const src = (await import("node:fs")).readFileSync(new URL("./AccessPassGate.tsx", import.meta.url), "utf8");
    for (const banned of [/\bSALT\b/, /reward/i, /\$\d/, /—/]) expect(src).not.toMatch(banned);
  });
});
