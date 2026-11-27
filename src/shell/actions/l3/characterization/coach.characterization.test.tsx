// 0.6.0 L3-WP00 characterization: the first-run coach and the toast as they are on citrate-core
// a826e441, pinned before L3-WP04/WP07 (checklist cards, coach retirement and honest copy) change
// them. A failure here means the coach or the toast changed: update the expectation only together
// with that reviewed change.
//
// The coach's Groups step claims "you already have a model running" even when the member skipped
// the model or it is still downloading. That false copy is pinned ON PURPOSE, so L3-WP07 has to
// change this expectation deliberately when it fixes the copy (AR-E4.6, F-11).
import { describe, it, expect } from "vitest";
import type { ReactElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { Coach, Toast } from "../../../Chrome";
import { COACH_STEPS } from "../../../../data/seed";
import { freshState, type AppState } from "../../../state";
import type { Store } from "../../../store";

// Coach and Toast render from `s`; `store` is touched only in onClick handlers.
const noopStore = {} as unknown as Store;
/** Static markup with React's text-node separators removed. */
const markup = (el: ReactElement) => renderToStaticMarkup(el).replace(/<!-- -->/g, "");

function done(over: Partial<AppState> = {}): AppState {
  return { ...freshState("p1"), stage: "done", queue: [], ...over };
}

describe("L3-WP00 characterization: coach and toast", () => {
  it("coach_groups_step_claims_model_running", () => {
    expect(COACH_STEPS.map((c) => c.id)).toEqual(["earn", "groups", "chat", "commissary"]);
    const groups = COACH_STEPS[1];
    expect(groups.title).toBe("Start a Group — you already have a model running");
    expect(groups.body).toContain("put your running model to work");
    expect(groups.cta).toEqual({ label: "Open your Groups", route: "groups" });
    // The claim shows whatever the model's real state is (here: not present, not skipped).
    const s = done({ coach: 1, modelState: "notPresent" });
    const html = markup(<Coach store={noopStore} s={s} />);
    expect(html).toContain("Start a Group — you already have a model running");
    expect(html).toContain("First run · 2 of 4");
    expect(html).toContain("Open your Groups");
  });

  it("coach_shows_only_after_onboarding_with_no_ceremony_queued", () => {
    const html = (s: AppState) => markup(<Coach store={noopStore} s={s} />);
    expect(html(done({ coach: 0 }))).toContain("First run · 1 of 4");
    expect(html(done({ coach: 3 }))).toContain("Done");
    // Not shown: coach off (-1), past the last step, before onboarding is done, or while a
    // Signature Ceremony request is queued.
    expect(html(done({ coach: -1 }))).toBe("");
    expect(html(done({ coach: 4 }))).toBe("");
    expect(html(done({ coach: 0, stage: "s6" }))).toBe("");
    const queued = done({ coach: 0 });
    queued.queue = [{} as AppState["queue"][number]];
    expect(html(queued)).toBe("");
  });

  it("toast_renders_its_text_only_when_set", () => {
    expect(markup(<Toast s={done({ toast: null })} />)).toBe("");
    const html = markup(<Toast s={done({ toast: "Node started" })} />);
    expect(html).toContain("Node started");
    // Plain text, rendered escaped: a toast never carries markup.
    expect(markup(<Toast s={done({ toast: "<b>x</b>" })} />)).toContain("&lt;b&gt;x&lt;/b&gt;");
  });
});
