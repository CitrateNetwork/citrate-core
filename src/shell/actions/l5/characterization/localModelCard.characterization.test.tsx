// 0.6.0 L5-WP01 characterization L5-CARD: the Settings › Models local model card as it is on
// citrate-core a826e441 (lane L5 evidence E12), pinned before L5 adds capability badges, companion
// downloads and the licence sheet to the Models surface. A failure here means the card's words
// changed: update the expectation only together with that reviewed change, in a separate commit.
import { describe, it, expect } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { LocalModelCard } from "../../../../surfaces/Models";
import { freshState, type AppState } from "../../../state";
import type { Store } from "../../../store";

// The card renders from `s`; `store` is touched only in click handlers.
const noopStore = {} as unknown as Store;
/** Static markup with React's text-node separators removed. */
const card = (s: AppState) => renderToStaticMarkup(<LocalModelCard store={noopStore} s={s} />).replace(/<!-- -->/g, "");

/** A third of the default model downloaded (33%). */
function st(over: Partial<AppState>): AppState {
  return { ...freshState("p1"), modelDownloadedBytes: 1_530_269_130, modelTotalBytes: 4_590_807_392, ...over };
}

/** The state word the card shows in its header. */
function stateWord(html: string): string | null {
  const m = html.match(/data-testid="models-local-model-state"[^>]*>([^<]*)</);
  return m ? m[1] : null;
}

const READY_LINE = "Verified against its pinned SHA-256. Chat can run on this computer.";

describe("L5-WP01 characterization: Settings local model card", () => {
  it("local_model_card_shows_six_states_with_todays_words", () => {
    const rows: [AppState["modelState"], string][] = [
      ["notPresent", "Not downloaded"],
      ["downloading", "Downloading 33%"],
      ["paused", "Paused at 33%"],
      ["verifying", "Verifying"],
      ["error", "Download stopped"],
      ["ready", "Ready"],
    ];
    for (const [modelState, word] of rows) {
      const html = card(st({ modelState }));
      expect(html, modelState).toContain('data-testid="models-local-model"');
      expect(stateWord(html), modelState).toBe(word);
      // The header names the chosen pinned model.
      expect(html, modelState).toContain("Gemma 4 E4B · Q4_0 · 4.59 GB");
    }
    // Before the real total is known, the percentage is of the pinned size.
    expect(stateWord(card(st({ modelState: "downloading", modelTotalBytes: 0 })))).toBe("Downloading 33%");
    // With the smaller model chosen, the header names it instead.
    expect(card(st({ modelState: "notPresent", modelChoice: "small" }))).toContain("Gemma 4 E2B · Q4_0 · 2.84 GB");
  });

  it("ready_card_says_verified_against_pinned_sha256", () => {
    const ready = card(st({ modelState: "ready" }));
    expect(ready).toContain(READY_LINE);
    // Ready shows no download controls.
    expect(ready).not.toContain('data-testid="model-download"');
    expect(ready).not.toContain('data-testid="model-pause"');
    // No other state claims verification.
    for (const modelState of ["notPresent", "downloading", "paused", "verifying", "error"] as const) {
      expect(card(st({ modelState })), modelState).not.toContain(READY_LINE);
    }
  });
});
