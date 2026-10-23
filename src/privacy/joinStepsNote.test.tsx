// 0.5.2 Join — the one-time note for members who turned telemetry on BEFORE 0.5.2, when the
// report gained the `joinSteps` field. Written red-first (JoinStepsNote and the consent marker
// did not exist).
//
// What these pin:
//   - a consent with no marker is treated as pre-0.5.2 (the marker is written on load);
//   - gating: nothing is recorded for a pre-0.5.2 opt-in until the note is acknowledged;
//   - a fresh opt-in (through the consent screen, which lists joinSteps) never sees the note;
//   - the note is shown once: after it is acknowledged it never comes back.
import { describe, it, expect, beforeEach, vi } from "vitest";
import { act, useState } from "react";
import { createRoot, type Root } from "react-dom/client";
import { JoinStepsNote } from "./JoinStepsNote";
import {
  reachedStep,
  pendingJoinSteps,
  setJoinFunnelConsent,
  funnelConsented,
  needsJoinStepsNote,
  telemetryConsentPatch,
  JOIN_STEPS_CONSENT_VERSION,
  PRE_052_CONSENT_VERSION,
} from "./joinFunnel";
import fields from "./telemetry-fields.json";
import { loadState, STORAGE_KEY, PERSIST_KEYS } from "../shell/state";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

beforeEach(() => {
  localStorage.clear();
  setJoinFunnelConsent(null);
});

async function mount(el: React.ReactElement): Promise<{ host: HTMLDivElement; root: Root }> {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => root.render(el));
  return { host, root };
}
const q = (host: HTMLElement, id: string) => host.querySelector(`[data-testid="${id}"]`) as HTMLElement | null;

describe("consent marker", () => {
  it("the disclosure version that added joinSteps is 2, and pre-0.5.2 is 1", () => {
    expect(fields.version).toBe(JOIN_STEPS_CONSENT_VERSION);
    expect(JOIN_STEPS_CONSENT_VERSION).toBe(2);
    expect(PRE_052_CONSENT_VERSION).toBe(1);
  });

  it("an existing consent with no marker is marked pre-0.5.2 on load; no consent stays unmarked", () => {
    localStorage.setItem(STORAGE_KEY, JSON.stringify({ persona: "p1", telemetry: true }));
    expect(loadState().telemetryConsentVersion).toBe(PRE_052_CONSENT_VERSION);
    localStorage.setItem(STORAGE_KEY, JSON.stringify({ persona: "p1", telemetry: false }));
    expect(loadState().telemetryConsentVersion).toBeNull();
    localStorage.setItem(STORAGE_KEY, JSON.stringify({ persona: "p1", telemetry: true, telemetryConsentVersion: 2 }));
    expect(loadState().telemetryConsentVersion).toBe(2);
    expect(PERSIST_KEYS).toContain("telemetryConsentVersion");
  });

  it("opting in through the consent screen records the current version; opting out clears it", () => {
    expect(telemetryConsentPatch(true)).toEqual({ telemetry: true, telemetryConsentVersion: 2 });
    expect(telemetryConsentPatch(false)).toEqual({ telemetry: false, telemetryConsentVersion: null });
  });
});

describe("gating before and after the note", () => {
  it("a pre-0.5.2 opt-in records nothing until the note is acknowledged", () => {
    expect(funnelConsented(true, null)).toBe(false);
    expect(funnelConsented(true, undefined)).toBe(false);
    expect(funnelConsented(true, 1)).toBe(false);
    expect(funnelConsented(true, 2)).toBe(true);
    expect(funnelConsented(false, 2)).toBe(false);

    let v: number | null = 1;
    setJoinFunnelConsent(() => funnelConsented(true, v));
    reachedStep("sign_in");
    expect(pendingJoinSteps()).toEqual([]);
    v = 2; // acknowledged
    reachedStep("sign_in");
    expect(pendingJoinSteps()).toEqual(["sign_in"]);
  });

  it("the persisted fallback applies the same rule", () => {
    localStorage.setItem(STORAGE_KEY, JSON.stringify({ telemetry: true }));
    reachedStep("wallet");
    expect(pendingJoinSteps()).toEqual([]);
    localStorage.setItem(STORAGE_KEY, JSON.stringify({ telemetry: true, telemetryConsentVersion: 2 }));
    reachedStep("wallet");
    expect(pendingJoinSteps()).toEqual(["wallet"]);
  });
});

describe("JoinStepsNote", () => {
  it("is not shown to a fresh opt-in, nor with telemetry off", async () => {
    expect(needsJoinStepsNote(true, 2)).toBe(false);
    expect(needsJoinStepsNote(false, null)).toBe(false);
    expect(needsJoinStepsNote(false, 1)).toBe(false);
    const fresh = await mount(<JoinStepsNote telemetry consentVersion={2} onAcknowledge={() => {}} onTurnOff={() => {}} />);
    expect(q(fresh.host, "join-steps-note")).toBeNull();
    const off = await mount(<JoinStepsNote telemetry={false} consentVersion={null} onAcknowledge={() => {}} onTurnOff={() => {}} />);
    expect(q(off.host, "join-steps-note")).toBeNull();
  });

  it("tells a pre-0.5.2 opt-in what joinSteps contains and does not, with a way to turn telemetry off", async () => {
    const onOff = vi.fn();
    const { host } = await mount(<JoinStepsNote telemetry consentVersion={1} onAcknowledge={() => {}} onTurnOff={onOff} />);
    const note = q(host, "join-steps-note");
    expect(note).not.toBeNull();
    const text = note?.textContent ?? "";
    expect(text).toContain("joinSteps");
    for (const s of fields.joinSteps) expect(text).toContain(s);
    for (const n of fields.joinStepsNeverIncluded) expect(text).toContain(n);
    await act(async () => q(host, "join-steps-note-off")?.click());
    expect(onOff).toHaveBeenCalledTimes(1);
  });

  it("is shown once: after it is acknowledged it does not come back, even after a restart", async () => {
    function Harness() {
      const [v, setV] = useState<number | null>(() => {
        try {
          return JSON.parse(localStorage.getItem("note-harness") ?? "null")?.v ?? 1;
        } catch {
          return 1;
        }
      });
      return (
        <JoinStepsNote
          telemetry
          consentVersion={v}
          onAcknowledge={() => {
            localStorage.setItem("note-harness", JSON.stringify({ v: JOIN_STEPS_CONSENT_VERSION }));
            setV(JOIN_STEPS_CONSENT_VERSION);
          }}
          onTurnOff={() => {}}
        />
      );
    }
    const first = await mount(<Harness />);
    expect(q(first.host, "join-steps-note")).not.toBeNull();
    await act(async () => q(first.host, "join-steps-note-ok")?.click());
    expect(q(first.host, "join-steps-note")).toBeNull();
    await act(async () => first.root.unmount());
    const second = await mount(<Harness />);
    expect(q(second.host, "join-steps-note")).toBeNull();
  });
});
