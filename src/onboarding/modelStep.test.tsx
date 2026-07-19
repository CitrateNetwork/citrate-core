// BC-3.3 (Rule 1 display honesty) — the S6.5 local-model step must render REAL
// status-driven progress, an honest SKIP path, and a "ready" state that appears
// ONLY from a real verify. The negative control: status is never "ready" without
// a verify — the enter button gates on a real ready (or an explicit skip), never
// on mere presence, and no fabricated "verified" caption is shown.
import { describe, it, expect, vi } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { ModelStep } from "./Onboarding";
import { freshState, type AppState } from "../shell/state";
import type { Store } from "../shell/store";

// The ModelStep renders purely from `s`; `store` is only touched in onClick
// handlers, which never fire during a static render.
const noopStore = {} as unknown as Store;

function modelState(over: Partial<AppState>): AppState {
  const s = freshState("p1");
  s.stage = "s6";
  s.node = "validating";
  return { ...s, ...over };
}

describe("S6.5 ModelStep — real status-driven progress (BC-3.3)", () => {
  it("renders a REAL byte-derived progress bar while downloading", () => {
    // 2,667,644,912 of 5,335,289,824 bytes = 50.0%. The bar + caption must reflect
    // the REAL bytes, not a fabricated number.
    const s = modelState({
      modelState: "downloading",
      modelDownloadedBytes: 2_667_644_912,
      modelTotalBytes: 5_335_289_824,
    });
    const html = renderToStaticMarkup(<ModelStep store={noopStore} s={s} />);
    expect(html).toContain("Downloading");
    expect(html).toContain("50%");
    // The GB caption traces to the real bytes (2.67 / 5.34 GB).
    expect(html).toContain("2.67");
    expect(html).toContain("5.34");
  });

  it("names the real source + the pinned SHA-256 (no fabricated 'verified')", () => {
    const s = modelState({ modelState: "notPresent" });
    const html = renderToStaticMarkup(<ModelStep store={noopStore} s={s} />);
    // The caption names the grounded HF source + the pinned checksum.
    expect(html).toContain("huggingface.co/ggml-org/gemma-4-E4B-it-GGUF");
    expect(html).toContain("90ce9812"); // the pinned sha256 prefix
    // notPresent shows NO verified/ready BADGE (the description prose may mention
    // "verified against a pinned SHA-256", but there is no fabricated ready state).
    expect(html).not.toContain("Local model verified — chat runs on-device");
  });

  it("shows the honest SKIP path routing chat to the gateway", () => {
    const s = modelState({ modelState: "notPresent", modelSkipped: true });
    const html = renderToStaticMarkup(<ModelStep store={noopStore} s={s} />);
    expect(html).toContain("Skipped");
    expect(html).toContain("gateway");
    // The Enter button is ENABLED after an honest skip.
    expect(html).not.toContain("disabled");
  });
});

describe("S6.5 ModelStep — the ready state only appears from a real verify (negative control)", () => {
  it("does NOT show a ready/verified badge while merely downloading (no verify yet)", () => {
    const s = modelState({
      modelState: "downloading",
      modelDownloadedBytes: 5_335_289_824,
      modelTotalBytes: 5_335_289_824,
    });
    const html = renderToStaticMarkup(<ModelStep store={noopStore} s={s} />);
    // Even at 100% downloaded, presence is NOT ready — no "verified" badge.
    expect(html).not.toContain("chat runs on-device");
    // The Enter button is DISABLED until a real verify (or a skip).
    expect(html).toContain("disabled");
  });

  it("shows the verified/ready badge ONLY when the state is a real 'ready'", () => {
    const s = modelState({ modelState: "ready" });
    const html = renderToStaticMarkup(<ModelStep store={noopStore} s={s} />);
    expect(html).toContain("Local model verified — chat runs on-device");
    // Enter is enabled only now (a real verify) — not disabled.
    expect(html).not.toContain("disabled");
  });

  it("an error state offers a retry and does NOT show ready (fail closed)", () => {
    const s = modelState({ modelState: "error", modelError: "model checksum mismatch — file quarantined (not ready)" });
    const html = renderToStaticMarkup(<ModelStep store={noopStore} s={s} />);
    expect(html).toContain("Retry download");
    expect(html).toContain("checksum mismatch");
    expect(html).not.toContain("chat runs on-device");
    // Enter is still gated (disabled) — an error never yields a usable local model.
    expect(html).toContain("disabled");
  });
});

// A store-level negative control: verifyModel must only reach "ready" from a real
// bridge.verify() success — a rejecting verify never sets ready (it fails closed to
// an error the UI shows honestly, routing chat to the gateway).
describe("store.verifyModel — ready is earned only from a real verify (Rule 1)", () => {
  it("a rejecting bridge.verify() sets error, NEVER ready", async () => {
    // A minimal store stub exposing just what verifyModel touches. We assert that a
    // failing verify never produces a "ready" state.
    const setStateCalls: Partial<AppState>[] = [];
    const stub = {
      state: { modelState: "verifying" } as AppState,
      setState(p: Partial<AppState>) {
        Object.assign(this.state, p);
        setStateCalls.push(p);
      },
      toast() {},
      save() {},
      // The private stopModelPoll is called internally; provide a no-op.
      stopModelPoll() {},
    };
    // Mock the bridge.model.verify to REJECT (a checksum mismatch).
    const bridgeMod = await import("../bridge");
    const spy = vi.spyOn(bridgeMod.bridge.model, "verify").mockRejectedValueOnce(new Error("checksum mismatch"));

    // Import the real method off the prototype and bind it to the stub.
    const { Store } = await import("../shell/store");
    await (Store.prototype.verifyModel as (this: typeof stub) => Promise<void>).call(stub);

    expect(stub.state.modelState).toBe("error");
    // It NEVER passed through a fabricated "ready".
    expect(setStateCalls.some((p) => p.modelState === "ready")).toBe(false);
    spy.mockRestore();
  });
});
