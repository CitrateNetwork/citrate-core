// HUP-S5.5 / S6.1: the Settings "Components" card is honest: with the signing key slot empty it
// says updates are off and why, every update button is disabled, the CVE SLA numbers are marked
// pending owner sign-off, and each tool's per-platform state is shown as measured or not.
import { describe, it, expect } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { ComponentUpdatesView, platformStateLabel } from "./ComponentUpdates";
import type { ComponentsStatus } from "../bridge/domains";

function status(over: Partial<ComponentsStatus> = {}): ComponentsStatus {
  return {
    keyConfigured: false,
    keyFingerprint: null,
    keyNote: "component updates are off: the component signing key is not configured yet (pending the key ceremony)",
    platform: "macos-arm64",
    freshness: "never_checked",
    manifestAgeSecs: null,
    browserMayOpenWeb: false,
    installed: [],
    bundle: [
      { name: "foundry", version: "1.5.1", license: "MIT OR Apache-2.0", measuredPlatforms: ["macos-arm64"], thisPlatform: "measured" },
      { name: "slither", version: "0.11.6", license: "AGPL-3.0", measuredPlatforms: [], thisPlatform: "to_be_built" },
      { name: "aderyn", version: "0.6.8", license: "GPL-3.0", measuredPlatforms: ["macos-arm64"], thisPlatform: "upstream_unavailable" },
    ],
    libraries: [{ name: "solady", sha256: "36991c81bbebc5155f5ab002ff2bed63eb845c1dec3257e3c7be66239f6b59ba" }],
    sla: { criticalHours: 72, highDays: 7, mediumDays: 30, lowDays: 90, staleAfterDays: 3, pendingOwnerSignoff: true },
    manifestUrl: "https://citrate-cdn.example/components/stable/manifest.json",
    ...over,
  };
}

const render = (s: ComponentsStatus | null) =>
  renderToStaticMarkup(<ComponentUpdatesView status={s} error={null} busy={null} onUpdate={() => {}} onRollback={() => {}} />);

describe("ComponentUpdatesView", () => {
  it("says updates are off while the key slot is empty, and disables every update button", () => {
    const html = render(status());
    expect(html).toContain("Updates are off");
    expect(html).toContain("key ceremony");
    expect(html).not.toMatch(/<button(?![^>]*disabled)[^>]*>Install/);
    expect(html).toContain("Nothing is installed yet");
  });

  it("marks the SLA values as pending owner sign-off", () => {
    const html = render(status());
    expect(html).toContain("72 h");
    expect(html).toContain("pending owner sign-off");
    const signed = render(status({ sla: { ...status().sla, pendingOwnerSignoff: false } }));
    expect(signed).not.toContain("pending owner sign-off");
  });

  it("shows each tool with an honest state for this machine", () => {
    const html = render(status());
    expect(html).toContain("foundry");
    expect(html).toContain(platformStateLabel("measured"));
    expect(html).toContain(platformStateLabel("to_be_built"));
    expect(html).toContain(platformStateLabel("upstream_unavailable"));
  });

  it("enables install only when the key is configured", () => {
    const html = render(status({ keyConfigured: true, keyFingerprint: "0011223344556677", keyNote: "signed" }));
    expect(html).toMatch(/<button(?![^>]*disabled)[^>]*>Install foundry/);
    // A tool with nothing measured for this machine stays disabled.
    expect(html).toMatch(/<button[^>]*disabled[^>]*>Install slither/);
  });

  it("offers rollback only for a component with a previous version", () => {
    const html = render(
      status({ keyConfigured: true, installed: [{ name: "foundry", version: "1.5.1", previous: "1.5.0", installedAt: 1 }] }),
    );
    expect(html).toContain("Roll back foundry to 1.5.0");
  });

  it("renders the web preview without inventing a store", () => {
    const html = render(null);
    expect(html).toContain("desktop app");
    expect(html).not.toContain("foundry");
  });

  it("uses no em-dashes in member-facing text", () => {
    expect(render(status())).not.toContain("—");
    expect(render(null)).not.toContain("—");
  });
});
