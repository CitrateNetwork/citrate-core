// @vitest-environment node
//
// HUP-S9.3 / RA-15 — the toolcall-v2 held-out manifest. Federated LoRA training drops any
// example whose user message normalizes to a held-out prompt, so the S9.4 eval delta is never
// measured on trained items. This test keeps the committed manifest equal to what the current v2
// dataset produces: adding a fragment without regenerating the manifest fails here.
//
// Regenerate: CITRATE_UPDATE_HELDOUT=1 npx vitest run src/agent/eval/heldout.test.ts, then copy
// the file byte for byte to citrate-compute-pool training-worker/fl-data/toolcall-v2.heldout.json
// and bump TOOLCALL_V2_HELDOUT_SHA256 there (training-worker/src/fl/dataset.rs).
import { describe, expect, it } from "vitest";
import { createHash } from "node:crypto";
import { readFileSync, readdirSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { loadToolcallDataset } from "./datasetFiles";
import { HELDOUT_FORMAT, buildHeldoutManifest, normalizePrompt, serializeHeldoutManifest } from "./heldout";

const DIR = resolve(process.cwd(), "src/agent/eval");
const FS = { readText: (p: string) => readFileSync(p, "utf8"), listDir: (p: string) => readdirSync(p) };
const MANIFEST = resolve(DIR, "toolcall-v2.heldout.json");
const sha = (s: string) => createHash("sha256").update(s, "utf8").digest("hex");

// The same vectors are asserted by compute-pool's Rust normalizer (dataset_tests.rs).
const VECTORS: [string, string][] = [
  ["What block height is my node at right now?", "what block height is my node at right now"],
  ["what block height is my node at, right now", "what block height is my node at right now"],
  ["  Héllo,\tWORLD!! 42 ", "héllo world 42"],
  ["tab\nnew-line_under", "tab new line under"],
  ["0x1111 → [REDACTED:email]", "0x1111 redacted email"],
  ["", ""],
];

describe("held-out normalization", () => {
  for (const [input, want] of VECTORS) {
    it(`normalizes ${JSON.stringify(input)}`, () => {
      expect(normalizePrompt(input)).toBe(want);
    });
  }

  it("hashes the normalized text (the vector compute-pool pins)", () => {
    expect(sha(normalizePrompt("What block height is my node at right now?"))).toBe(
      sha("what block height is my node at right now"),
    );
  });
});

// compute-pool pins the same value (PARITY_V1_TOOLS in training-worker/src/fl/dataset.rs): the
// trainer rows name the 29 parity-v1 tools, so a renamed or added tool fails one side until both
// are updated.
const PARITY_V1_TOOL_NAMES_SHA256 = "15534eb4d24830b58acb575a01cf6e7070e7ffb9ac81a463e8fe69350b3eecee";

describe("parity-v1 tool names (the trainer's tool schema)", () => {
  it("are the 29 names compute-pool trains against", () => {
    const parity = JSON.parse(readFileSync(resolve(process.cwd(), "src/agent/parity/parity-v1.json"), "utf8")) as {
      tools: { name: string }[];
    };
    expect(parity.tools).toHaveLength(29);
    expect(sha(parity.tools.map((t) => t.name).join("\n"))).toBe(PARITY_V1_TOOL_NAMES_SHA256);
  });
});

describe("toolcall-v2 held-out manifest", () => {
  const ds = loadToolcallDataset(FS, DIR, "v2");
  const built = serializeHeldoutManifest(buildHeldoutManifest(ds, sha));

  it("covers every toolcall-v2 item once, sorted by id", () => {
    const m = JSON.parse(built) as { format: string; dataset: string; count: number; items: { id: string }[] };
    expect(m.format).toBe(HELDOUT_FORMAT);
    expect(m.dataset).toBe("toolcall-v2");
    expect(m.count).toBe(ds.tasks.length);
    expect(new Set(m.items.map((i) => i.id)).size).toBe(ds.tasks.length);
    const ids = m.items.map((i) => i.id);
    expect([...ids].sort()).toEqual(ids);
  });

  it("carries no prompt text, only hashes", () => {
    for (const t of ds.tasks) expect(built).not.toContain(t.prompt);
  });

  it("is the committed file, byte for byte", () => {
    if (process.env.CITRATE_UPDATE_HELDOUT === "1") writeFileSync(MANIFEST, built);
    expect(readFileSync(MANIFEST, "utf8")).toBe(built);
  });
});
