// 0.5.4 WP-H6: the dataset store bridge. The Tauri side passes each call straight to core's
// fl_dataset_* command with the argument names the Rust command takes; the web preview refuses
// every call (no sealed store there) instead of inventing a dataset.
import { readFileSync } from "node:fs";
import { describe, expect, it, vi, beforeEach } from "vitest";

const invokeMock = vi.fn(async (_cmd: string, _args?: Record<string, unknown>) => ({}));
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (cmd: string, args?: Record<string, unknown>) => invokeMock(cmd, args),
  isTauri: () => true,
}));

import { tauriFlDatasets } from "../bridge/tauri/flDatasets";
import { DATASETS_NEED_DESKTOP, simFlDatasets } from "../bridge/sim/flDatasets";
import type { SimHost } from "../bridge/sim";

const ops = [{ op: "dedupe" as const }, { op: "redact" as const, find: "a", replace: "b" }];

describe("tauriFlDatasets", () => {
  beforeEach(() => invokeMock.mockClear());

  it("calls core's commands with the Rust argument names", async () => {
    await tauriFlDatasets.list();
    await tauriFlDatasets.manifest("ds-0123456789abcdef", 2);
    await tauriFlDatasets.create({ name: "Notes", dataClass: "notes", source: { kind: "folder", path: "/home/m/notes" }, consentStatement: "Use these files." });
    await tauriFlDatasets.sample("ds-0123456789abcdef", 1);
    await tauriFlDatasets.cleanPreview("ds-0123456789abcdef", 1, ops);
    await tauriFlDatasets.cleanApply("ds-0123456789abcdef", 1, ops, "0x" + "ab".repeat(32));
    await tauriFlDatasets.consentList("ds-0123456789abcdef");
    await tauriFlDatasets.consentWithdraw("ds-0123456789abcdef", "c-2");
    await tauriFlDatasets.delete("ds-0123456789abcdef");
    expect(invokeMock.mock.calls).toEqual([
      ["fl_dataset_list", undefined],
      ["fl_dataset_manifest", { dataset: "ds-0123456789abcdef", version: 2 }],
      ["fl_dataset_create", { request: { name: "Notes", dataClass: "notes", source: { kind: "folder", path: "/home/m/notes" }, consentStatement: "Use these files." } }],
      ["fl_dataset_sample", { dataset: "ds-0123456789abcdef", version: 1, n: null, offset: null }],
      ["fl_dataset_clean_preview", { dataset: "ds-0123456789abcdef", version: 1, ops }],
      ["fl_dataset_clean_apply", { dataset: "ds-0123456789abcdef", version: 1, ops, resultContentHash: "0x" + "ab".repeat(32) }],
      ["fl_dataset_consent_list", { dataset: "ds-0123456789abcdef" }],
      ["fl_dataset_consent_withdraw", { dataset: "ds-0123456789abcdef", consent: "c-2" }],
      ["fl_dataset_delete", { dataset: "ds-0123456789abcdef" }],
    ]);
  });

  it("every command the bridge calls is in the main window's ACL and registered in lib.rs", () => {
    const acl = readFileSync("src-tauri/permissions/main-window.toml", "utf8");
    const lib = readFileSync("src-tauri/src/lib.rs", "utf8");
    const src = readFileSync("src/bridge/tauri/flDatasets.ts", "utf8");
    const cmds = [...src.matchAll(/invoke(?:<[^>]+>)?\("(fl_dataset_[a-z_]+)"/g)].map((m) => m[1]);
    expect(cmds.length).toBe(9);
    for (const c of cmds) {
      expect(acl).toContain(`"${c}"`);
      expect(lib).toContain(`fl_datasets::${c},`);
    }
  });
});

describe("simFlDatasets", () => {
  it("refuses every call: the web preview has no dataset store", async () => {
    const sim = simFlDatasets({} as SimHost);
    const calls = [
      sim.list(),
      sim.manifest("ds-0123456789abcdef", 1),
      sim.create({ name: "n", dataClass: "c", source: { kind: "conversations", max: 5 }, consentStatement: "s" }),
      sim.sample("ds-0123456789abcdef", 1),
      sim.cleanPreview("ds-0123456789abcdef", 1, ops),
      sim.cleanApply("ds-0123456789abcdef", 1, ops, "0x"),
      sim.consentList("ds-0123456789abcdef"),
      sim.consentWithdraw("ds-0123456789abcdef", "c-1"),
      sim.delete("ds-0123456789abcdef"),
    ];
    for (const c of calls) await expect(c).rejects.toThrow(DATASETS_NEED_DESKTOP);
  });
});
