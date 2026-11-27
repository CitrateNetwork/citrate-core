// 0.6.0 L3-WP00: the L3 characterization suite, listed in one place.
//
// L3 (shell, onboarding, nav, notifications) pins today's launch behavior before any 0.6.0 change
// to it: the gate order, the onboarding stages, the routes, the coach copy, the toast, the absence
// of OS notifications and what app quit does. Each listed test is green on citrate-core a826e441
// and must only change together with a reviewed, deliberate change of its expectation.
//
// This file is the lane's list until LQ's shared characterization registry
// (.agentile/coverage/characterization.json, LQ-WP12) lands; LQ copies the SUITE entries from here.
// It fails when a listed file or test is missing, so the suite cannot shrink silently.
import { describe, it, expect } from "vitest";
import { existsSync, readFileSync } from "node:fs";

interface SuiteFile {
  surface: string;
  runner: "vitest" | "cargo";
  file: string;
  tests: string[];
}

export const SUITE: SuiteFile[] = [
  {
    surface: "launch-gates",
    runner: "vitest",
    file: "src/shell/gateOrder.characterize.test.tsx",
    tests: [
      "update_gate_renders_nothing_else_when_required",
      "network_compat_gate_mounts_after_body",
      "auth_gate_for_signed_out_past_s1_in_tauri",
      "no_auth_gate_on_s0_s1",
      "access_pass_gate_wraps_onboarding_only",
    ],
  },
  {
    surface: "onboarding-stages",
    runner: "vitest",
    file: "src/onboarding/stages.characterize.test.tsx",
    tests: [
      "seven_stage_spine_labels",
      "s6_model_step_hidden_until_peers_gt_0",
      "s6_continue_goes_s7_with_grant_else_done",
      "on_explore_sets_done_and_coachDone",
      "on_enter_starts_coach_when_not_done",
    ],
  },
  {
    surface: "launch",
    runner: "vitest",
    file: "src/shell/launch.characterize.test.ts",
    tests: [
      "node_autostarts_regardless_of_stage_when_intent_run",
      "resume_session_runs_after_custody_unlock",
      "s3_resume_from_chain_on_launch",
      "signed_in_fields_not_persisted",
    ],
  },
  {
    surface: "nav",
    runner: "vitest",
    file: "src/shell/places.characterize.test.ts",
    tests: ["six_places_in_order", "canonical_route_count", "every_legacy_route_resolves", "unknown_route_falls_back_to_hermes_chat"],
  },
  {
    surface: "coach-toast",
    runner: "vitest",
    file: "src/shell/coach.characterize.test.tsx",
    tests: ["coach_groups_step_claims_model_running", "coach_shows_only_after_onboarding_with_no_ceremony_queued", "toast_renders_its_text_only_when_set"],
  },
  {
    surface: "notifications",
    runner: "vitest",
    file: "src/notify/none.characterize.test.ts",
    tests: ["no_notification_plugin_in_cargo_or_capabilities", "comms_copy_claims_os_notification"],
  },
  {
    surface: "app-lifecycle",
    runner: "cargo",
    file: "src-tauri/tests/lifecycle_characterize.rs",
    tests: ["exit_requested_stops_all_sidecars", "no_window_close_or_exit_is_intercepted"],
  },
];

describe("L3 characterization suite (L3-WP00)", () => {
  it("lists every surface once, each with at least one test", () => {
    const surfaces = SUITE.map((f) => f.surface);
    expect(new Set(surfaces).size).toBe(surfaces.length);
    for (const f of SUITE) expect(f.tests.length, f.file).toBeGreaterThan(0);
  });

  for (const f of SUITE) {
    it(`${f.file} exists and defines every listed test`, () => {
      expect(existsSync(f.file), f.file).toBe(true);
      const src = readFileSync(f.file, "utf8");
      for (const t of f.tests) {
        const defined = f.runner === "cargo" ? src.includes(`fn ${t}()`) : src.includes(`it("${t}"`);
        expect(defined, `${f.file}: ${t}`).toBe(true);
      }
    });
  }
});
