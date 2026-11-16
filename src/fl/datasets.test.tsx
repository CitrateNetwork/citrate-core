// FL design board 08, Rounds › Datasets: one test per StateMatrix state this build can reach
// (Empty: no datasets yet + Build; Not deployed: DatasetRegistry record waits; Success: a set built
// with its hash; Failure: scrape refused, publish blocked; Unavailable: the store was not read).
// Core's fl_trajectories.rs answers are faked in its shape.
import { describe, expect, it, vi } from "vitest";
import { act } from "react";
import { createRoot } from "react-dom/client";
import realBook from "../../src-tauri/addresses/40204.json";
import type { FlRoundsDomain, FlTrainingSet, FlTrajectoryStatus } from "../bridge/domains";
import { readFlNetworkState } from "./networkState";
import { DatasetsView } from "./Datasets";

const reads = { codeSize: async () => 1000, viewCall: async () => "0x" + "0".repeat(64) };
const status = (enabled: boolean, datasets = 0): FlTrajectoryStatus => ({ settings: { enabled, changedAtMs: null }, recordedFiles: 3, datasets, appliesOnRestart: false, loadError: null });
const SET: FlTrainingSet = { path: "/data/fl/trajectories/set-1.jsonl", sha256: "9f3c".padEnd(64, "a"), examples: 42, filesRead: 3, skippedLines: 1, overCap: 0 };

function domain(over: Partial<FlRoundsDomain> = {}): FlRoundsDomain {
  return { trajectoryStatus: vi.fn(async () => status(false)), setTrajectoryConsent: vi.fn(async () => status(true)), buildTrainingSet: vi.fn(async () => SET), ...over } as unknown as FlRoundsDomain;
}

async function mount(fl: FlRoundsDomain) {
  const state = await readFlNetworkState(realBook, reads);
  const el = document.createElement("div");
  document.body.appendChild(el);
  const root = createRoot(el);
  await act(async () => root.render(<DatasetsView state={state} fl={fl} toast={() => {}} go={() => {}} />));
  const click = async (t: string) => {
    const b = [...document.querySelectorAll("button")].find((x) => x.textContent?.trim() === t);
    if (!b) throw new Error("no button " + t);
    await act(async () => b.click());
  };
  return { el, click, done: () => (act(() => root.unmount()), el.remove()) };
}

describe("Datasets states (StateMatrix)", () => {
  it("Empty: No datasets yet, with Build", async () => {
    const m = await mount(domain());
    expect(m.el.querySelector('[data-testid="datasets-empty"]')!.textContent).toContain("No datasets yet");
    expect(m.el.textContent).toContain("Local first. Nothing is shared unless you publish a version.");
    m.done();
  });
  it("Not deployed: publishing waits for DatasetRegistry, and there is no publish button", async () => {
    const m = await mount(domain());
    await m.click("Publish");
    expect(m.el.querySelector('[data-testid="publish-unavailable"]')!.textContent).toContain("DatasetRegistry: Not available yet on this network");
    expect([...m.el.querySelectorAll("button")].map((b) => b.textContent)).not.toContain("Sign and publish");
    m.done();
  });
  it("Clean with Hermes says it is not in this build", async () => {
    const m = await mount(domain());
    await m.click("Clean with Hermes");
    expect(m.el.querySelector('[data-testid="clean-unavailable"]')!.textContent).toContain("Cleaning with Hermes is not in this build");
    m.done();
  });
  it("Failure: collecting from a site is refused with the robots.txt and license rule", async () => {
    const m = await mount(domain());
    await m.click("Build a dataset");
    await m.click("Collect from a site");
    const d = document.querySelector('[role="dialog"]')!;
    expect(d.querySelector('[role="alert"]')!.textContent).toContain("robots.txt disallows collection, or that states no license, is refused");
    expect(d.querySelector<HTMLButtonElement>('[data-testid="ds-build"]')!.disabled).toBe(true);
    await m.click("Local files");
    expect(d.textContent).toContain("Building from a folder on this device is not in this build");
    m.done();
  });
  it("Hermes conversations: Build stays off until consent is on (off by default)", async () => {
    const m = await mount(domain());
    await m.click("Build a dataset");
    const d = document.querySelector('[role="dialog"]')!;
    expect(d.textContent).toContain("Training on them needs your consent, which is off by default.");
    expect(d.querySelector<HTMLButtonElement>('[data-testid="ds-build"]')!.disabled).toBe(true);
    m.done();
  });
  it("Success: with consent on, a set is built and shown with its rows and hash", async () => {
    const fl = domain({ trajectoryStatus: vi.fn(async () => status(true, 0)) });
    const m = await mount(fl);
    await m.click("Build a dataset");
    const btn = document.querySelector<HTMLButtonElement>('[data-testid="ds-build"]')!;
    expect(btn.disabled).toBe(false);
    await act(async () => btn.click());
    expect(fl.buildTrainingSet).toHaveBeenCalledWith(500);
    expect(document.querySelector('[role="dialog"]')).toBeNull();
    expect(m.el.textContent).toContain("Verified Hermes conversations");
    expect(m.el.textContent).toContain("42");
    expect(m.el.textContent).toContain(SET.sha256);
    expect(m.el.textContent).toContain("Redacted, verified conversations only");
    m.done();
  });
  it("Failure: core's refusal to build is shown, nothing is invented", async () => {
    const fl = domain({ trajectoryStatus: vi.fn(async () => status(true, 0)), buildTrainingSet: vi.fn(async () => Promise.reject(new Error("no verified conversations recorded yet"))) });
    const m = await mount(fl);
    await m.click("Build a dataset");
    await act(async () => document.querySelector<HTMLButtonElement>('[data-testid="ds-build"]')!.click());
    expect(m.el.textContent).toContain("no verified conversations recorded yet");
    m.done();
  });
  it("Unavailable: a failed store read is said, never 'no datasets'", async () => {
    const m = await mount(domain({ trajectoryStatus: vi.fn(async () => Promise.reject(new Error("store locked"))) }));
    expect(m.el.textContent).toContain("store locked");
    expect(m.el.querySelector('[data-testid="datasets-empty"]')).toBeNull();
    m.done();
  });
});
