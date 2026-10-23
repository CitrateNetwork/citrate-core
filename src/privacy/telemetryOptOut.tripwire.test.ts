// Tripwire (0.5.2 Join, review of #365): every change to the telemetry toggle goes through
// `setTelemetryConsent` in joinFunnel.ts, which is the one place that forgets unsent join steps
// and invalidates prepared or in-flight reports on opt-out. A new opt-out path that writes the
// toggle directly would skip both, so this scan fails CI on one.
import { describe, it, expect } from "vitest";
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative } from "node:path";

const SRC = join(__dirname, "..");

function files(dir: string): string[] {
  return readdirSync(dir).flatMap((n) => {
    const p = join(dir, n);
    if (statSync(p).isDirectory()) return files(p);
    return /\.(ts|tsx)$/.test(n) && !/\.test\.(ts|tsx)$/.test(n) ? [p] : [];
  });
}

// Files that mention a `telemetry:` key without writing the toggle, and why.
const ALLOWED: Record<string, string> = {
  "privacy/joinFunnel.ts": "owns setTelemetryConsent and the consent patch",
  "privacy/JoinStepsNote.tsx": "a prop type",
  "shell/state.ts": "the AppState type and its default (off)",
  "bridge/types.ts": "the AppConfig type and its default (off)",
  "bridge/domains.ts": "the bridge domain map",
  "bridge/cx.ts": "the bridge domain map",
  "bridge/sim/index.ts": "reads the toggle back for config.read",
};

describe("tripwire: the telemetry toggle is only changed through setTelemetryConsent", () => {
  it("no file writes `telemetry:` or uses the raw consent patch outside the allowlist", () => {
    const offenders: string[] = [];
    for (const f of files(SRC)) {
      const rel = relative(SRC, f).split("\\").join("/");
      if (ALLOWED[rel]) continue;
      const src = readFileSync(f, "utf8");
      src.split("\n").forEach((line, i) => {
        if (/\btelemetryConsentPatch\(|\brememberTelemetryConsent\(|\bclearJoinSteps\(/.test(line)) offenders.push(`${rel}:${i + 1}: ${line.trim()}`);
        const m = /\btelemetry\s*:\s*([^,}\n]+)/.exec(line);
        // The only allowed shape outside the allowlist: the value of a setTelemetryConsent patch.
        if (m && !/^\w+\.telemetry\b/.test(m[1].trim())) offenders.push(`${rel}:${i + 1}: ${line.trim()}`);
        if (m && /^\w+\.telemetry\b/.test(m[1].trim()) && !/=\s*setTelemetryConsent\(/.test(src)) offenders.push(`${rel}:${i + 1}: ${line.trim()} (no setTelemetryConsent in file)`);
      });
    }
    expect(offenders).toEqual([]);
  });

  it("every allowlisted file still exists (no stale entries)", () => {
    for (const rel of Object.keys(ALLOWED)) expect(() => statSync(join(SRC, rel))).not.toThrow();
  });
});
