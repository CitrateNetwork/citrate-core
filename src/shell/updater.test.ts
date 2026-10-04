import { describe, it, expect } from "vitest";
import {
  formatBytes,
  isCriticalNotes,
  progressFraction,
  INITIAL_UPDATE_STATE,
  isNoPlatformInFeed,
} from "./updater";

describe("updater — pure helpers", () => {
  it("formatBytes is human + honest at boundaries", () => {
    expect(formatBytes(0)).toBe("0 B");
    expect(formatBytes(-5)).toBe("0 B");
    expect(formatBytes(512)).toBe("512 B");
    expect(formatBytes(1024)).toBe("1 KB");
    expect(formatBytes(1536)).toBe("1.5 KB");
    expect(formatBytes(5 * 1024 * 1024)).toBe("5 MB");
    expect(formatBytes(2 * 1024 * 1024 * 1024)).toBe("2 GB");
  });

  it("isCriticalNotes only fires on an explicit opt-in marker", () => {
    expect(isCriticalNotes(null)).toBe(false);
    expect(isCriticalNotes("")).toBe(false);
    expect(isCriticalNotes("Fixes a typo")).toBe(false);
    expect(isCriticalNotes("[critical] security fix")).toBe(true);
    expect(isCriticalNotes("CRITICAL: patch the grant path")).toBe(true);
    // "critical" mid-sentence must NOT trip it (avoid false auto-download).
    expect(isCriticalNotes("this is a non-critical polish release")).toBe(false);
  });

  it("progressFraction is null until a real total is known, then clamped 0..1", () => {
    expect(progressFraction({ downloadedBytes: 10, totalBytes: null })).toBeNull();
    expect(progressFraction({ downloadedBytes: 0, totalBytes: 0 })).toBeNull();
    expect(progressFraction({ downloadedBytes: 50, totalBytes: 200 })).toBeCloseTo(0.25);
    // never report >100% even if the plugin over-counts
    expect(progressFraction({ downloadedBytes: 300, totalBytes: 200 })).toBe(1);
  });

  it("initial state is an honest idle (no fabricated available update)", () => {
    expect(INITIAL_UPDATE_STATE.status).toBe("idle");
    expect(INITIAL_UPDATE_STATE.version).toBeNull();
    expect(INITIAL_UPDATE_STATE.critical).toBe(false);
  });
});

describe("updater — feed without this platform", () => {
  it("treats a feed that does not list this OS/arch as no update, not an error", () => {
    // Exact tauri-plugin-updater text, as seen on Linux and Windows builds.
    expect(
      isNoPlatformInFeed(
        'None of the fallback platforms `["linux-aarch64-appimage", "linux-aarch64"]` were found in the response `platforms` object',
      ),
    ).toBe(true);
    expect(
      isNoPlatformInFeed(
        'None of the fallback platforms `["windows-x86_64-nsis", "windows-x86_64"]` were found in the response `platforms` object',
      ),
    ).toBe(true);
    expect(isNoPlatformInFeed("the platform `windows-x86_64` was not found on the response `platforms` object")).toBe(true);
  });

  it("still surfaces real failures", () => {
    expect(isNoPlatformInFeed("error sending request for url (https://citrate.ai/updater/latest.json)")).toBe(false);
    expect(isNoPlatformInFeed("signature verification failed")).toBe(false);
    expect(isNoPlatformInFeed("Could not fetch a valid release JSON from the remote")).toBe(false);
    expect(isNoPlatformInFeed("")).toBe(false);
  });
});
