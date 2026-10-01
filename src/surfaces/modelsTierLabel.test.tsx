// HUP-S1.6 — the Models surface labels an already-present model that matches the tier in effect
// as recommended for this machine. Nothing is downloaded and no catalog entry is invented.
import { describe, it, expect, afterEach } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { Models } from "./Models";
import { modelsSlice } from "../shell/slices/models";
import { tierSlice } from "../shell/slices/tier";
import { sampleReport } from "../shell/slices/tierTestReport";
import { freshState } from "../shell/state";
import type { Store } from "../shell/store";
import type { ModelDescriptor } from "../bridge/domains";

const gemma: ModelDescriptor = {
  id: "local:gemma-4-E4B-it-Q4_0.gguf",
  source: "bundled",
  repo: "",
  file: "gemma-4-E4B-it-Q4_0.gguf",
  sizeBytes: 4_590_807_392,
  sha256: "",
  kind: "gguf",
};

const render = () => renderToStaticMarkup(<Models store={{} as unknown as Store} s={freshState("p1")} />);

afterEach(() => {
  modelsSlice.set({ local: [] });
  tierSlice.set({ report: null, loaded: false });
});

describe("HUP-S1.6 Models — tier recommended label", () => {
  it("labels a local model of the effective tier as recommended", () => {
    modelsSlice.set({ local: [gemma] });
    tierSlice.set({ report: sampleReport({ effective: "T0" }), loaded: true });
    expect(render()).toContain("recommended for this machine (T0)");
  });

  it("no label when the local model belongs to another tier", () => {
    modelsSlice.set({ local: [gemma] });
    tierSlice.set({ report: sampleReport(), loaded: true }); // effective T2
    expect(render()).not.toContain("recommended for this machine");
  });

  it("no label without a tier report (never guessed)", () => {
    modelsSlice.set({ local: [gemma] });
    expect(render()).not.toContain("recommended for this machine");
  });
});
