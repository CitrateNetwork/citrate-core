// HUP-S3.3 + S3.7 — onboarding offers the persona choice as an optional step. Skipping it keeps
// Hermes's default voice, and the step says the choice can change later in Settings.
import { describe, it, expect } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { PersonaStep } from "./Onboarding";
import { freshState } from "../shell/state";
import type { Store } from "../shell/store";

const noopStore = {} as unknown as Store;

describe("onboarding PersonaStep", () => {
  it("is optional, starts on the default voice and points to Settings", () => {
    const s = { ...freshState("p1"), stage: "s6" as const, node: "validating" as const };
    const html = renderToStaticMarkup(<PersonaStep store={noopStore} s={s} />);
    expect(html).toContain("Optional");
    expect(html).toContain("Hermes (default voice)");
    expect(html).toContain("change this later in Settings");
    expect(html).toMatch(/placeholders, pending owner sign-off/);
    // compact: no custom-persona form during onboarding
    expect(html).not.toContain("persona-custom-open");
  });
});
