// HUP-S0.2 — deadlines per command class. The 12 s backstop pre-empted legitimately long work:
// a local model turn (bounded Rust-side at AI_REQUEST_TIMEOUT = 300 s) and a catalog model
// download (drives its own progress) were rejected in the UI while Rust kept working, so replies
// were lost and a retry corrupted the download's .part file.
import { describe, it, expect } from "vitest";
import { deadlineFor, INVOKE_TIMEOUT_MS } from "./invoke";

describe("HUP-S0.2 invoke deadlines", () => {
  it("chat turns get a deadline ABOVE the Rust-side 300 s request bound", () => {
    for (const c of ["ai_chat", "ai_chat_tools", "ai_chat_local", "ai_chat_local_tools"]) {
      expect(deadlineFor(c)).toBeGreaterThan(300_000);
      expect(Number.isFinite(deadlineFor(c))).toBe(true);
    }
  });

  it("catalog downloads and large file work are never pre-empted", () => {
    for (const c of ["model_catalog_download", "model_download", "storage_add", "storage_retrieve"]) {
      expect(deadlineFor(c)).toBe(Infinity);
    }
  });

  it("Hermes control calls outlive the Rust-side 30 s control bound", () => {
    for (const c of ["hermes_status", "hermes_run_skill", "hermes_resolve"]) {
      expect(deadlineFor(c)).toBeGreaterThan(30_000);
    }
  });

  it("everything else keeps the 12 s backstop", () => {
    expect(deadlineFor("node_status")).toBe(INVOKE_TIMEOUT_MS);
    expect(deadlineFor("groups_list")).toBe(INVOKE_TIMEOUT_MS);
  });
});

describe("HUP-S0.1b deadlines for the newly-async signing and stop commands", () => {
  it("broadcast outlives its 60 s receipt poll (a timeout mid-broadcast would misreport a sent tx)", () => {
    expect(deadlineFor("sign_and_broadcast")).toBeGreaterThan(60_000);
  });
  it("stop commands outlive their 10 s graceful-shutdown wait", () => {
    for (const c of ["node_stop", "memory_stop", "ipfs_stop", "model_serve_stop", "agent_stop"]) {
      expect(deadlineFor(c)).toBeGreaterThan(10_000);
    }
  });
});
