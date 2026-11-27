// 0.6.0 L3-WP00 characterization: what a Tauri launch does today (store.start() on citrate-core
// a826e441), pinned before L3-WP01/WP02 (background node, persistent session) change it. The launch
// path runs only in a Tauri build, so it is pinned from the source of `start()`, plus the state
// defaults it reads. A failure here means the launch changed: update the expectation only together
// with that reviewed change.
//
//   1. The node autostarts on every launch while the member's intent is "run", at any stage.
//   2. The signed-in session is resumed only after the custody vault unlock settles.
//   3. A launch that lands on S3 asks the chain whether the grant already landed.
//   4. The signed-in identity fields are never persisted (the real Store.save(), sentinel values).
import { describe, it, expect, beforeEach, vi } from "vitest";
import { readFileSync } from "node:fs";
import { PERSIST_KEYS, STORAGE_KEY, freshState, type AppState } from "../../../state";
import { store } from "../../../store";

const STORE = readFileSync("src/shell/store.ts", "utf8");

/** The body of `start()` in store.ts, from its signature to its closing brace. */
function startBody(): string {
  const open = STORE.indexOf("\n  start(): void {\n");
  expect(open, "store.ts defines start()").toBeGreaterThan(0);
  const close = STORE.indexOf("\n  }\n", open + 1);
  expect(close).toBeGreaterThan(open);
  return STORE.slice(open, close);
}
const flat = (s: string) => s.replace(/\s+/g, " ");

/** The tauri-only part of start(): from the first `if (BRIDGE_MODE === "tauri") {` block that polls the node. */
function tauriBlock(): string {
  const body = startBody();
  const at = body.indexOf('if (BRIDGE_MODE === "tauri") {\n      // CORE-D3');
  expect(at, "the tauri launch block").toBeGreaterThan(0);
  return body.slice(at);
}

const SIGNED_IN_FIELDS = ["signedIn", "authSub", "authEmail", "authName", "authInitials", "authExpiresAt"];

describe("L3-WP00 characterization: launch (store.start)", () => {
  beforeEach(() => {
    try {
      localStorage.clear();
    } catch {
      /* jsdom */
    }
  });

  it("node_autostarts_regardless_of_stage_when_intent_run", () => {
    expect(freshState("p1").nodeIntent).toBe("run");
    const block = tauriBlock();
    const auto = /setTimeout\(\(\) => \{([^}]*)\}, 1500\);/.exec(block);
    expect(auto, "the deferred autostart").not.toBeNull();
    expect(flat(auto![1]).trim()).toBe(
      'if (this.state.nodeIntent !== "run") return; if (this.state.node !== "off" && this.state.node !== "error") return; this.startNode();',
    );
    // Nothing about the onboarding stage gates the autostart.
    expect(auto![1]).not.toContain("stage");
    // It is the only place the launch starts the node.
    expect(block.split("this.startNode()").length - 1).toBe(1);
  });

  it("resume_session_runs_after_custody_unlock", () => {
    const body = flat(startBody());
    const unlock = body.indexOf("void this.custodyEnsureUnlocked().finally(() => {");
    expect(unlock).toBeGreaterThan(0);
    const resume = body.indexOf("void this.resumeSession();");
    expect(resume).toBeGreaterThan(unlock);
    // Called once, inside the unlock's finally (before the wallet reads that follow it).
    expect(body.split("this.resumeSession()").length - 1).toBe(1);
    expect(body.slice(unlock, resume)).not.toContain("});");
    expect(body.indexOf("void this.refreshWallet();", unlock)).toBeGreaterThan(resume);
  });

  it("s3_resume_from_chain_on_launch", () => {
    expect(tauriBlock()).toContain('if (this.state.stage === "s3") void this.settleS3IfAlreadyGranted();');
  });

  it("signed_in_fields_not_persisted", () => {
    for (const k of SIGNED_IN_FIELDS) expect(PERSIST_KEYS as string[], k).not.toContain(k);
    // The real save, run on a signed-in session whose identity fields hold sentinel values: the
    // stored entry carries none of the six fields and none of the values, under any key.
    // Built from one short marker so no line reads like an assigned credential to the secret scan.
    const mark = "l3wp00x";
    const sentinels = {
      signedIn: true,
      authSub: mark + "-sub",
      authEmail: mark + "@example.invalid",
      authName: mark + " Name",
      authInitials: mark + "QZ",
      authExpiresAt: "2099-01-02T03:04:05.000Z",
    };
    const prev = store.state;
    vi.useFakeTimers();
    try {
      localStorage.clear();
      store.state = { ...freshState("p1"), stage: "done", ...sentinels } as AppState;
      store.save();
      vi.advanceTimersByTime(1000);
      const raw = localStorage.getItem(STORAGE_KEY);
      expect(raw, "Store.save() wrote the entry").not.toBeNull();
      const saved = JSON.parse(raw as string) as Record<string, unknown>;
      for (const k of SIGNED_IN_FIELDS) expect(Object.keys(saved), k).not.toContain(k);
      for (const v of Object.values(sentinels)) if (typeof v === "string") expect(raw, v).not.toContain(v);
      // Nothing else in storage carries them either.
      for (let i = 0; i < localStorage.length; i++) {
        const k = localStorage.key(i) as string;
        for (const v of Object.values(sentinels)) if (typeof v === "string") expect(localStorage.getItem(k), k).not.toContain(v);
      }
    } finally {
      vi.useRealTimers();
      store.state = prev;
      localStorage.clear();
    }
  });
});
