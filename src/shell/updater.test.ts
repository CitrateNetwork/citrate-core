import { describe, it, expect } from "vitest";
import {
  formatBytes,
  isCriticalNotes,
  progressFraction,
  INITIAL_UPDATE_STATE,
  isNoPlatformInFeed,
  applyStagedUpdate,
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

describe("CORE-D6: the downloaded update is installed only after the sidecars are drained", () => {
  type Step = "drain" | "cancel" | "begin" | "install" | "relaunch";
  const report = (allStopped: boolean) => ({
    allStopped,
    tookMs: 1200,
    services: [
      { id: "node", label: "Node", before: "alive", after: allStopped ? "notSupervised" : "stopping", stopped: allStopped },
      { id: "memory", label: "Memory", before: "alive", after: "notSupervised", stopped: true },
    ],
  });
  function steps(over: Partial<Record<Step, () => Promise<unknown>>> = {}) {
    const calls: string[] = [];
    const mk = (n: Step, dflt: () => Promise<unknown>) => async (arg?: unknown) => {
      calls.push(arg === true ? `${n}:installFailed` : n);
      return (over[n] ?? dflt)();
    };
    return {
      calls,
      s: {
        drain: mk("drain", async () => report(true)) as () => Promise<ReturnType<typeof report>>,
        cancel: mk("cancel", async () => undefined) as (installFailed?: boolean) => Promise<void>,
        beginInstall: mk("begin", async () => undefined) as () => Promise<void>,
        install: mk("install", async () => undefined) as () => Promise<void>,
        relaunch: mk("relaunch", async () => undefined) as () => Promise<void>,
      },
    };
  }

  it("drains, then installs, then relaunches", async () => {
    const { calls, s } = steps();
    await expect(applyStagedUpdate(s)).resolves.toEqual({ ok: true });
    expect(calls).toEqual(["drain", "begin", "install", "relaunch"]);
  });

  it("never installs while a sidecar is still running; releases the hold and names it", async () => {
    const { calls, s } = steps({ drain: async () => report(false) });
    const r = await applyStagedUpdate(s);
    expect(calls).toEqual(["drain", "cancel"]);
    expect(r).toEqual({ ok: false, error: expect.stringContaining("Node (stopping) did not stop") });
  });

  it("a failed drain or a failed install releases the hold and says so; no relaunch", async () => {
    const a = steps({ drain: async () => { throw new Error("internal: managed state unavailable"); } });
    const ra = await applyStagedUpdate(a.s);
    expect(a.calls).toEqual(["drain", "cancel"]);
    expect(ra.ok ? "" : ra.error).toMatch(/services could not be stopped \(internal: managed state unavailable\)/);

    const b = steps({ install: async () => { throw new Error("signature verification failed"); } });
    const rb = await applyStagedUpdate(b.s);
    expect(b.calls).toEqual(["drain", "begin", "install", "cancel:installFailed"]);
    expect(rb.ok ? "" : rb.error).toMatch(/could not be installed: signature verification failed\. Services stay stopped/);
  });

  it("a cancel that itself fails does not hide the reason", async () => {
    const { s } = steps({ drain: async () => report(false), cancel: async () => { throw new Error("x"); } });
    const r = await applyStagedUpdate(s);
    expect(r.ok ? "" : r.error).toMatch(/did not stop/);
  });

  it("#614 review: a rejected relaunch keeps the services stopped and says the update is installed", async () => {
    const { calls, s } = steps({ relaunch: async () => { throw new Error("restart refused"); } });
    const r = await applyStagedUpdate(s);
    expect(calls).toEqual(["drain", "begin", "install", "relaunch"]);
    expect(r).toEqual({ ok: false, keepStopped: true, error: expect.stringMatching(/The update is installed.*quit and reopen Citrate Core.*restart refused/s) });
  });

  it("#614 review: an install that does not finish in time keeps the services stopped and says what to do", async () => {
    const { calls, s } = steps({ install: () => new Promise<void>(() => undefined) });
    const r = await applyStagedUpdate(s, { installTimeoutMs: 30 });
    expect(calls).toEqual(["drain", "begin", "install"]);
    expect(r).toEqual({ ok: false, keepStopped: true, error: expect.stringMatching(/did not finish installing.*Quit and reopen Citrate Core/s) });
  });

  it("#614 review: a drain that reports a child still exiting never installs", async () => {
    const { calls, s } = steps({ drain: async () => ({ ...report(true), allStopped: false, stillExiting: ["citrate-node"] }) });
    const r = await applyStagedUpdate(s);
    expect(calls).toEqual(["drain", "cancel"]);
    expect(r.ok ? "" : r.error).toMatch(/citrate-node is still exiting/);
  });

  it("#614 review: the hook releases a hold left by an earlier page, and shows a kept-stopped result as installed", async () => {
    const fs = await import("node:fs");
    const path = await import("node:path");
    const src = fs.readFileSync(path.resolve(__dirname, "updater.ts"), "utf8");
    expect(src).toMatch(/useEffect\(\(\) => \{\s*if \(BRIDGE_MODE !== "tauri"\) return;\s*\/\/ #614 review[^\n]*\n[^\n]*update_prepare_cancel/);
    expect(src).toMatch(/r\.keepStopped \? "installed" : "error"/);
    const banner = fs.readFileSync(path.resolve(__dirname, "UpdateBanner.tsx"), "utf8");
    expect(banner).toMatch(/state\.status === "installed"/);
  });

  it("#617 review: the install is marked begun before it runs; if that is refused nothing installs", async () => {
    const { calls, s } = steps({ begin: async () => { throw new Error("the services were not drained"); } });
    const r = await applyStagedUpdate(s);
    expect(calls).toEqual(["drain", "begin", "cancel"]);
    expect(r.ok ? "" : r.error).toMatch(/not installed.*the services were not drained/);
  });

  it("#617 review: a page reload keeps the hold once an install began, and shows the installed state", async () => {
    const fs = await import("node:fs");
    const path = await import("node:path");
    const src = fs.readFileSync(path.resolve(__dirname, "updater.ts"), "utf8");
    // The mount cancel is a plain cancel (no installFailed): Rust refuses it after an install began.
    expect(src).toMatch(/invoke<void>\("update_prepare_cancel", \{\}\)[\s\S]{0,200}INSTALL_HELD/);
    expect(src).toMatch(/beginInstall: \(\) => invoke<void>\("update_install_begin", \{\}\)/);
    expect(src).toMatch(/cancel: \(installFailed\?: boolean\) => invoke<void>\("update_prepare_cancel", \{ installFailed: installFailed \?\? null \}\)/);
  });

  it("the hook downloads with the plugin's download() and installs only from restart (source check)", async () => {
    const fs = await import("node:fs");
    const path = await import("node:path");
    const src = fs.readFileSync(path.resolve(__dirname, "updater.ts"), "utf8");
    expect(src).not.toMatch(/downloadAndInstall\(/);
    expect(src).toMatch(/await update\.download\(/);
    expect(src).toMatch(/install: \(\) => update\.install\(\)/);
    expect(src).toMatch(/invoke<DrainReport>\("update_prepare"/);
  });
});
