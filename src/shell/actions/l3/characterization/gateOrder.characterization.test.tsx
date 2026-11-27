// 0.6.0 L3-WP00 characterization: the launch gate order in App.tsx, as it is on citrate-core
// a826e441, pinned before L3-WP01/WP02 (first run, AuthGate extraction, persistent session) change
// it. A failure here means the gate order changed: update the expectation only together with that
// reviewed change (lane L3 §4, "the gate order is frozen").
//
//   1. GateOrApp: while an update is required only the update gate renders, nothing of the app.
//   2. Inside GateOrApp the body comes first and NetworkCompatGate mounts after it.
//   3. The Tauri auth gate covers a signed-out session past s0/s1, and nothing else.
//   4. AccessPassGate wraps onboarding only, never the Shell.
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { readFileSync } from "node:fs";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";

vi.mock("../../../../bridge", () => ({ bridge: { mode: "tauri", shell: { openExternal: async () => {} } } }));

import { GateOrApp, type AppUpdate } from "../../../UpdateBanner";
import { INITIAL_UPDATE_STATE, withVerdict, type UpdateState } from "../../../updater";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const APP = readFileSync("src/App.tsx", "utf8");
/** App.tsx with every run of whitespace collapsed to one space, so the pins ignore line wrapping. */
const FLAT = APP.replace(/\s+/g, " ");

/**
 * What App.tsx's Root renders outside GateOrApp, as a list of problems (empty when nothing does).
 * JSX comments are dropped first; whitespace is collapsed so line wrapping does not matter.
 */
function outsideGate(app: string): string[] {
  const flat = app.replace(/\{\/\*[\s\S]*?\*\/\}/g, " ").replace(/\s+/g, " ");
  const problems: string[] = [];
  if (flat.split("function Root()").length !== 2) return ["App.tsx must define Root once"];
  if (flat.split("<GateOrApp ").length !== 2 || flat.split("</GateOrApp>").length !== 2) problems.push("GateOrApp must appear once");
  const root = flat.slice(flat.indexOf("function Root()"), flat.indexOf("function App()"));
  const open = root.indexOf("<GateOrApp update={update}>");
  const close = root.indexOf("</GateOrApp>");
  const ret = root.lastIndexOf("return (", open);
  if (open < 0 || close < open || ret < 0) return [...problems, "Root returns GateOrApp"];
  const before = root.slice(ret + "return (".length, open).trim();
  if (!/^<div style=\{\{[^{}]*\}\} data-register="charter">$/.test(before)) problems.push("before the gate: " + before);
  const after = root.slice(close + "</GateOrApp>".length).trim();
  if (after !== "</div> ); }") problems.push("after the gate: " + after);
  return problems;
}

/** The JSX between `<GateOrApp update={update}>` and `</GateOrApp>`. */
function insideGate(): string {
  const open = FLAT.indexOf("<GateOrApp update={update}>");
  const close = FLAT.indexOf("</GateOrApp>");
  expect(open).toBeGreaterThan(0);
  expect(close).toBeGreaterThan(open);
  return FLAT.slice(open, close);
}

/** The `needsAuth` expression from App.tsx, evaluated for one launch state. */
function needsAuth(mode: "tauri" | "sim", signedIn: boolean, stage: string): boolean {
  const m = /const needsAuth = ([^;]+);/.exec(FLAT);
  expect(m, "App.tsx defines needsAuth").not.toBeNull();
  const expr = m![1];
  const fn = new Function("bridge", "s", `return (${expr});`) as (b: { mode: string }, s: { signedIn: boolean; stage: string }) => boolean;
  return fn({ mode }, { signedIn, stage });
}

const STAGES = ["s0", "s1", "s2", "s3", "s4", "s5", "s6", "s7", "done"];

const gated = (over: Partial<UpdateState> = {}): UpdateState => ({
  ...withVerdict(INITIAL_UPDATE_STATE, { currentVersion: "0.5.3", minimumVersion: "0.5.4", updateRequired: true, reason: null, refused: null }),
  ...over,
});
const update = (state: UpdateState): AppUpdate => ({
  state,
  check: vi.fn(async () => {}),
  install: vi.fn(async () => {}),
  restart: vi.fn(async () => {}),
  dismiss: vi.fn(),
});

let el: HTMLDivElement;
let root: Root;
beforeEach(() => {
  el = document.createElement("div");
  document.body.appendChild(el);
  root = createRoot(el);
});
afterEach(() => {
  act(() => root.unmount());
  el.remove();
});

describe("L3-WP00 characterization: launch gate order (App.tsx)", () => {
  it("update_gate_renders_nothing_else_when_required", async () => {
    await act(async () =>
      root.render(
        <GateOrApp update={update(gated({ status: "ready", version: "0.5.4" }))}>
          <div data-app="">app</div>
        </GateOrApp>,
      ),
    );
    expect(el.querySelector("[data-app]")).toBeNull();
    expect(el.querySelector("[data-update-gate]")).not.toBeNull();

    await act(async () =>
      root.render(
        <GateOrApp update={update(INITIAL_UPDATE_STATE)}>
          <div data-app="">app</div>
        </GateOrApp>,
      ),
    );
    expect(el.querySelector("[data-app]")).not.toBeNull();
    expect(el.querySelector("[data-update-gate]")).toBeNull();

    // Root wraps its whole tree in the gate: its return is the styled outer div holding only
    // GateOrApp, so nothing (no element, no native button, no {expression}) sits beside the gate.
    expect(outsideGate(APP)).toEqual([]);
  });

  it("outside_gate_check_catches_content_beside_the_gate", () => {
    // The pin above is a source check, so it is shown failing on the shapes it must catch.
    const gate = "<GateOrApp update={update}>";
    const mutants = [
      APP.replace(gate, "<button>Wallet</button>\n      " + gate),
      APP.replace(gate, "{body}\n      " + gate),
      APP.replace("</GateOrApp>", "</GateOrApp>\n      <Toast s={s} />"),
      APP.replace("</GateOrApp>", "</GateOrApp>\n      {s.toast}"),
    ];
    for (const m of mutants) {
      expect(m).not.toBe(APP);
      expect(outsideGate(m).length, m.slice(m.indexOf("function Root()"), m.indexOf("function App()"))).toBeGreaterThan(0);
    }
  });

  it("network_compat_gate_mounts_after_body", () => {
    const inside = insideGate();
    // Today's mount order inside GateOrApp, first to last.
    const order = [
      "{body}",
      "<SignatureCeremony store={store} s={s} />",
      "<WalletReviewModal store={store} s={s} />",
      "<Coach store={store} s={s} />",
      "<DemoPanel store={store} s={s} />",
      "<Toast s={s} />",
      "<JoinStepsNote",
      "<SignedForYou />",
      "<NetworkCompatGate />",
      "<UpdateBanner update={update} />",
    ];
    let at = -1;
    for (const part of order) {
      const i = inside.indexOf(part);
      expect(i, part).toBeGreaterThan(at);
      at = i;
    }
    // Each mounts exactly once.
    for (const part of order) expect(inside.split(part).length - 1, part).toBe(1);
  });

  it("auth_gate_for_signed_out_past_s1_in_tauri", () => {
    expect(FLAT).toContain('const needsAuth = bridge.mode === "tauri" && !s.signedIn && s.stage !== "s0" && s.stage !== "s1";');
    // The body: the auth gate first, then onboarding until "done", then the Shell.
    expect(FLAT).toContain(
      'const body = needsAuth ? ( <AuthGate /> ) : s.stage !== "done" ? ( <AccessPassGate store={store} s={s}> <Onboarding store={store} s={s} /> </AccessPassGate> ) : ( <Shell s={s} /> );',
    );
    for (const stage of ["s2", "s3", "s4", "s5", "s6", "s7", "done"]) {
      expect(needsAuth("tauri", false, stage), stage).toBe(true);
      expect(needsAuth("tauri", true, stage), stage).toBe(false);
    }
    // web-dev (sim) has no real auth, so no gate at any stage.
    for (const stage of STAGES) {
      expect(needsAuth("sim", false, stage), stage).toBe(false);
      expect(needsAuth("sim", true, stage), stage).toBe(false);
    }
    // The gate's copy says the app signs out on every launch (WP02 changes this on purpose).
    expect(FLAT).toContain("Citrate Core signs out on every launch.");
    expect(FLAT).toContain("session never stored on disk");
  });

  it("no_auth_gate_on_s0_s1", () => {
    for (const stage of ["s0", "s1"]) {
      expect(needsAuth("tauri", false, stage), stage).toBe(false);
      expect(needsAuth("tauri", true, stage), stage).toBe(false);
    }
  });

  it("access_pass_gate_wraps_onboarding_only", () => {
    expect(FLAT.split("<AccessPassGate ").length - 1).toBe(1);
    const open = FLAT.indexOf("<AccessPassGate store={store} s={s}>");
    const close = FLAT.indexOf("</AccessPassGate>");
    expect(open).toBeGreaterThan(0);
    expect(FLAT.slice(open, close + "</AccessPassGate>".length)).toBe(
      "<AccessPassGate store={store} s={s}> <Onboarding store={store} s={s} /> </AccessPassGate>",
    );
    // The Shell is rendered on its own, outside the access pass gate.
    expect(FLAT.indexOf("<Shell s={s} />")).toBeGreaterThan(close);
  });
});
