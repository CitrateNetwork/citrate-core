// 2026-11-22 (owner: "the user can access a decent local model on the way into the app"): the
// model step and Settings › Models render every download state from the real status: progress
// with speed and ETA, waiting for the connection, paused, the actual error with Retry, a disk too
// small, and the smaller-model choice. Static renders: `store` is touched only in click handlers.
import { describe, it, expect } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { ModelStep } from "./Onboarding";
import { LocalModelControls } from "./LocalModelControls";
import { LocalModelCard } from "../surfaces/Models";
import { freshState, type AppState } from "../shell/state";
import type { Store } from "../shell/store";

const noopStore = {} as unknown as Store;

function st(over: Partial<AppState>): AppState {
  const s = freshState("p1");
  s.stage = "s6";
  return { ...s, ...over };
}

const step = (s: AppState) => renderToStaticMarkup(<ModelStep store={noopStore} s={s} />);
const controls = (s: AppState) => renderToStaticMarkup(<LocalModelControls store={noopStore} s={s} offerSkip={true} />);
const card = (s: AppState) => renderToStaticMarkup(<LocalModelCard store={noopStore} s={s} />);

describe("model download: error state", () => {
  it("shows the actual error in plain words with Retry and the smaller model", () => {
    const msg = "The model servers could not be reached (connection refused). Your progress is saved; Retry continues where it stopped.";
    const html = step(st({ modelState: "error", modelError: msg }));
    expect(html).toContain('data-testid="model-error"');
    expect(html).toContain("connection refused");
    expect(html).toContain("Retry download");
    expect(html).toContain("Choose a smaller model (2.84 GB)");
    expect(html).toContain("Skip for now, use the gateway");
  });
});

describe("model download: downloading", () => {
  it("shows real bytes, speed, time left and Pause", () => {
    const html = controls(
      st({ modelState: "downloading", modelDownloadedBytes: 1_530_269_130, modelTotalBytes: 4_590_807_392, modelBytesPerSec: 12_500_000, modelEtaSecs: 245 }),
    );
    expect(html).toContain("33.3%");
    expect(html).toContain("1.53 / 4.59 GB");
    expect(html).toContain("13 MB/s");
    expect(html).toContain("about 4 min left");
    expect(html).toContain('data-testid="model-pause"');
  });

  it("says it is waiting for the connection, and why, while it retries", () => {
    const html = controls(
      st({ modelState: "downloading", modelDownloadedBytes: 10, modelTotalBytes: 4_590_807_392, modelRetryInSecs: 42, modelRetryReason: "The connection stopped sending data for 45 seconds." }),
    );
    expect(html).toContain("Waiting for the connection. Trying again in 42s.");
    expect(html).toContain("stopped sending data");
  });
});

describe("model download: paused", () => {
  it("shows where it paused, that progress is saved, Resume and the smaller model", () => {
    const html = step(st({ modelState: "paused", modelDownloadedBytes: 2_295_403_696, modelTotalBytes: 4_590_807_392 }));
    expect(html).toContain("Paused at 50%");
    expect(html).toContain("your progress is saved");
    expect(html).toContain('data-testid="model-resume"');
    expect(html).toContain('data-testid="model-choose-smaller"');
  });
});

describe("model download: disk and the smaller model", () => {
  it("names the free space when the default does not fit, and offers the smaller model when it does", () => {
    const html = step(st({ modelState: "notPresent", modelFreeBytes: 3_500_000_000 }));
    expect(html).toContain('data-testid="model-low-disk"');
    expect(html).toContain("This disk has 3.50 GB free");
    expect(html).toContain("The smaller model may fit.");
    expect(html).toContain("Choose a smaller model (2.84 GB)");
  });

  it("with the smaller model chosen, the step shows its own pinned facts and a way back", () => {
    const html = step(st({ modelState: "notPresent", modelChoice: "small", modelSkipped: false }));
    expect(html).toContain("Download the local model (2.84 GB)");
    expect(html).toContain("huggingface.co/ggml-org/gemma-4-E2B-it-GGUF");
    expect(html).toContain("8e30dff3");
    expect(html).toContain("Use Gemma 4 E4B instead (4.59 GB)");
    expect(html).not.toContain('data-testid="model-choose-smaller"');
  });

  it("the default model is still the default", () => {
    const html = step(st({ modelState: "notPresent" }));
    expect(html).toContain("Download the local model (4.59 GB)");
    expect(html).toContain("Download Gemma 4 E4B (4.59 GB)");
  });
});

describe("Settings, Models: the local model is always available there", () => {
  it.each([
    [{ modelState: "notPresent" } as Partial<AppState>, "Not downloaded", 'data-testid="model-download"'],
    [{ modelState: "downloading", modelDownloadedBytes: 1_561_000_000, modelTotalBytes: 4_590_807_392 } as Partial<AppState>, "Downloading 34%", 'data-testid="model-pause"'],
    [{ modelState: "paused", modelDownloadedBytes: 1_560_874_513, modelTotalBytes: 4_590_807_392 } as Partial<AppState>, "Paused at 34%", 'data-testid="model-resume"'],
    [{ modelState: "error", modelError: "Not enough free disk space." } as Partial<AppState>, "Download stopped", "Retry download"],
    [{ modelState: "ready" } as Partial<AppState>, "Ready", "Verified against its pinned SHA-256"],
  ])("%o reads %s", (over, label, action) => {
    const s = st({ ...over, stage: "done", modelSkipped: true });
    const html = card(s);
    expect(html).toContain(label);
    expect(html).toContain(action);
    expect(html).not.toContain("Skip for now");
  });
});

describe("copy rules", () => {
  it("the new download copy has no em-dashes", () => {
    for (const s of [
      st({ modelState: "error", modelError: "x." }),
      st({ modelState: "paused", modelDownloadedBytes: 1, modelTotalBytes: 2 }),
      st({ modelState: "downloading", modelDownloadedBytes: 1, modelTotalBytes: 2, modelRetryInSecs: 3, modelRetryReason: "r." }),
      st({ modelState: "notPresent", modelFreeBytes: 1 }),
    ]) {
      expect(controls(s)).not.toContain("—");
      expect(card({ ...s, stage: "done" })).not.toContain("—");
    }
  });
});
