// Q-A.4b item 8 — the Journal sidebar caption overstated: journal pages persist
// to localStorage, not an encrypted data-dir file. The caption must be softened to
// the truth ("stored locally on this device") and must NOT claim encryption-at-rest
// that never happens.
import { describe, it, expect } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { Journal } from "./Journal";
import { freshState, type AppState } from "../shell/state";
import type { Store } from "../shell/store";

// Journal renders from `s`; `store` is only touched in handlers (never fired by a
// static render).
const noopStore = {} as unknown as Store;

function journalState(): AppState {
  const s = freshState("p1");
  s.route = "journal";
  return s;
}

describe("Journal caption — Q-A.4b item 8 honesty", () => {
  it("does NOT claim 'encrypted at rest in your data dir' (pages persist to localStorage)", () => {
    const html = renderToStaticMarkup(<Journal store={noopStore} s={journalState()} />);
    // NEGATIVE CONTROL — the overstated encryption-at-rest claim is gone.
    expect(html).not.toContain("encrypted at rest in your data dir");
    // POSITIVE — the honest, softened framing.
    expect(html.toLowerCase()).toContain("stored locally on this device");
  });
});
