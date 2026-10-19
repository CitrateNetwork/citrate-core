// 0.5.2 Join — anonymous step-reached funnel events, carried by the EXISTING opt-in telemetry.
// Written red-first: joinFunnel.ts and the joinSteps bundle field did not exist.
//
// What these pin:
//   - gating: with telemetry off nothing is recorded, nothing is pending, nothing is invoked;
//   - allowlist: a step record is a bare step id, and the bundle carries exactly the reviewed
//     keys (snapshot) — no invite code, group, address, email, cluster name or time;
//   - dedupe: one record per install per step, and a step already sent is never sent again.
import { describe, it, expect, beforeEach, vi } from "vitest";
import {
  JOIN_STEPS,
  reachedStep,
  pendingJoinSteps,
  markJoinStepsSent,
  clearJoinSteps,
  setJoinFunnelConsent,
  stepForStage,
  JOIN_FUNNEL_STORAGE_KEY,
} from "./joinFunnel";
import fields from "./telemetry-fields.json";
import { simTelemetry } from "../bridge/sim/telemetry";
import type { SimHost } from "../bridge/sim";

const invokeMock = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...a: unknown[]) => invokeMock(...a) }));

let consent = false;
beforeEach(() => {
  localStorage.clear();
  invokeMock.mockReset();
  consent = false;
  setJoinFunnelConsent(() => consent);
});

describe("join funnel — gating on the telemetry toggle", () => {
  it("records nothing while telemetry is off", () => {
    for (const s of JOIN_STEPS) reachedStep(s);
    expect(pendingJoinSteps()).toEqual([]);
    expect(localStorage.getItem(JOIN_FUNNEL_STORAGE_KEY)).toBeNull();
  });

  it("never sends on its own, on or off (sending stays the reviewed report)", () => {
    reachedStep("sign_in");
    consent = true;
    reachedStep("kyc");
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("hides recorded steps once telemetry is turned off, and clearing wipes them", () => {
    consent = true;
    reachedStep("wallet");
    expect(pendingJoinSteps()).toEqual(["wallet"]);
    consent = false;
    expect(pendingJoinSteps()).toEqual([]);
    clearJoinSteps();
    expect(localStorage.getItem(JOIN_FUNNEL_STORAGE_KEY)).toBeNull();
    consent = true;
    expect(pendingJoinSteps()).toEqual([]);
  });

  it("falls back to the persisted toggle when no live source is bound (default off)", () => {
    setJoinFunnelConsent(null);
    reachedStep("arrive");
    expect(pendingJoinSteps()).toEqual([]);
    localStorage.setItem("citrate-core-proto-v2", JSON.stringify({ telemetry: true, telemetryConsentVersion: 2 }));
    reachedStep("arrive");
    expect(pendingJoinSteps()).toEqual(["arrive"]);
  });
});

describe("join funnel — payload allowlist", () => {
  it("has exactly the eleven join steps, in path order, matching the disclosure", () => {
    expect([...JOIN_STEPS]).toMatchInlineSnapshot(`
      [
        "arrive",
        "download",
        "open_app",
        "sign_in",
        "kyc",
        "membership",
        "wallet",
        "bond",
        "node_synced",
        "activate_validator",
        "join_group",
      ]
    `);
    expect(fields.joinSteps).toEqual([...JOIN_STEPS]);
    expect(fields.fields.map((f) => f.key)).toContain("joinSteps");
  });

  it("ignores anything that is not an allowlisted step id", () => {
    consent = true;
    for (const bad of ["citrate://join/AB12CD", "0x2655d9fbbe599e75ff6e53790f99ebc9a20c93bf", "me@example.com", "group:42", ""]) {
      reachedStep(bad as never);
    }
    expect(pendingJoinSteps()).toEqual([]);
    expect(localStorage.getItem(JOIN_FUNNEL_STORAGE_KEY)).toBeNull();
  });

  it("stores only bare step ids (no time, no id, no context)", () => {
    consent = true;
    reachedStep("bond");
    reachedStep("arrive");
    const raw = JSON.parse(localStorage.getItem(JOIN_FUNNEL_STORAGE_KEY) ?? "null");
    expect(raw).toMatchInlineSnapshot(`
      {
        "pending": [
          "arrive",
          "bond",
        ],
        "sent": [],
        "v": 1,
      }
    `);
  });

  it("a reviewed bundle carries exactly the known keys (snapshot)", async () => {
    const b = await simTelemetry({} as SimHost).bundle(["e"], ["sign_in", "kyc"]);
    expect(Object.keys(b).sort()).toMatchInlineSnapshot(`
      [
        "appVersion",
        "crashTail",
        "joinSteps",
        "nodeLogTail",
        "os",
        "reportId",
        "uiErrors",
      ]
    `);
    expect(b.joinSteps).toEqual(["sign_in", "kyc"]);
  });

  it("the desktop bridge hands Rust only the error ring and the step ids", async () => {
    const { tauriTelemetry } = await import("../bridge/tauri/telemetry");
    invokeMock.mockResolvedValue({});
    await tauriTelemetry.bundle(["e"], ["wallet"]);
    expect(invokeMock).toHaveBeenCalledWith("diagnostics_bundle", { uiErrors: ["e"], joinSteps: ["wallet"] });
  });
});

describe("join funnel — dedupe per install per step", () => {
  it("records a step once however often it is reached, in path order", () => {
    consent = true;
    reachedStep("kyc");
    reachedStep("sign_in");
    reachedStep("kyc");
    reachedStep("sign_in");
    expect(pendingJoinSteps()).toEqual(["sign_in", "kyc"]);
  });

  it("a step already sent in a report is never queued again", () => {
    consent = true;
    reachedStep("sign_in");
    reachedStep("kyc");
    markJoinStepsSent(["sign_in"]);
    expect(pendingJoinSteps()).toEqual(["kyc"]);
    reachedStep("sign_in");
    expect(pendingJoinSteps()).toEqual(["kyc"]);
  });

  it("survives unreadable storage without throwing", () => {
    consent = true;
    localStorage.setItem(JOIN_FUNNEL_STORAGE_KEY, "{not json");
    expect(() => reachedStep("wallet")).not.toThrow();
    expect(pendingJoinSteps()).toEqual(["wallet"]);
  });
});

describe("join funnel — onboarding stage hook points", () => {
  it("maps each onboarding stage to the step it reaches", () => {
    expect(["s0", "s1", "s2", "s3", "s4", "s5", "s6", "done"].map(stepForStage)).toEqual([
      null,
      "sign_in",
      "kyc",
      "membership",
      "wallet",
      "bond",
      null,
      null,
    ]);
  });
});
