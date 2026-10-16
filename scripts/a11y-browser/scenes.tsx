// =====================================================================
// HUP-S10.6 follow-up — the scenes the real-browser accessibility pass renders.
//
// Not app code: built only by scripts/a11y-browser.mjs into a temporary folder and opened in
// Chromium, where axe-core runs with real layout and the app's real stylesheet, so the
// colour-contrast rule (off under jsdom) judges real pixels. Each scene is the same component
// and state the jsdom tests in src/a11y render, plus the US-7.4 AC1 monitor sections.
// `#<scene id>` picks the scene; `window.__sceneIds` lists them.
// =====================================================================
import { StrictMode, type ReactElement } from "react";
import { createRoot } from "react-dom/client";
import "../../src/styles/index.css";
import { ActivityMonitor } from "../../src/popout/ActivityMonitor";
import { PopoutRoot } from "../../src/popout/PopoutRoot";
import { buildMonitorSnapshot, daemonsSection, type MonitorInputs } from "../../src/popout/monitorSnapshot";
import type { BridgeTransport } from "../../src/popout/bridge";
import { IDLE_ACTIVITY, type TurnActivity } from "../../src/shell/slices/turnActivity";
import { SignatureCeremony, WalletReviewModal } from "../../src/shell/Chrome";
import { DeployGateCard } from "../../src/shell/DeployGateCard";
import { freshState, type AppState, type CerSpec } from "../../src/shell/state";
import type { Store } from "../../src/shell/store";
import { chainCard, commandCard, diffCard, fieldsCard } from "../../src/agent/approvalCards";
import { GATE_ITEM_IDS, type DeployGateRecord } from "../../src/agent/deployGate";
import type { CeremonyView } from "../../src/bridge/types";

const inputs: MonitorInputs = {
  activity: IDLE_ACTIVITY,
  providerKind: "local",
  providerLabel: "local model · llama-server · agentic",
  modelLabel: "Gemma 4 E4B",
  modelId: "local:gemma",
  tier: "T0",
  localCtxTokens: 8192,
  now: 10_000,
};
const running: TurnActivity = {
  ...IDLE_ACTIVITY,
  state: "running",
  providerKind: "sidecar",
  providerLabel: "Hermes (sidecar loop · preview)",
  phase: "tool",
  currentTool: "node_status",
  step: 2,
  startedAt: 1_000,
  tools: [
    { id: "c0", name: "memory_search", state: "done", startedAt: 1_500, endedAt: 2_000 },
    { id: "c1", name: "node_status", state: "running", startedAt: 3_000, endedAt: null },
  ],
  usage: { promptTokens: 3000, completionTokens: 96, generationMs: 3200, calls: 2 },
  plan: ["write-test", "implement", "review"],
  approvals: [
    { callId: "c2", tool: "gsheets_append", state: "pending", at: 4_000 },
    { callId: "c3", tool: "shell_run", state: "declined", at: 5_000 },
  ],
  verifiers: [
    { step: "write-test", name: "forge_tests_pass", passed: true, detail: "", at: 6_000 },
    { step: "implement", name: "slither", passed: false, detail: "1 high finding", at: 7_000 },
  ],
};
const daemon = (id: string, lastTokenSource: string | null, running: boolean) => ({
  id,
  name: "Daemon " + id,
  prompt: "x",
  schedule: "0 9 * * *",
  budget: { maxRunsPerDay: 4, maxTokensPerDay: 20_000, maxTokensPerRun: 6_000, maxSpendSalt: "0" },
  paused: false,
  status: (running ? "running" : "scheduled") as "running" | "scheduled",
  running,
  runsToday: 1,
  tokensToday: 720,
  skippedToday: 0,
  spendTodaySalt: "0",
  nextRunMs: 1_790_845_200_000,
  lastRunMs: null,
  lastOutcome: "answered",
  lastNote: null,
  lastTokenSource,
});

const noop = () => {};
function fakeStore() {
  return { finishCer: noop, toast: noop, approveCer: noop, rejectWalletReview: async () => {}, approveWalletReview: async () => {}, setWalletReviewRawAck: noop } as unknown as Store;
}
const write = { effect: "write", trust: "trusted" } as const;
const base: CerSpec = { origin: "chat agent", requester: "dashboard agent · tool x", title: "Write a skill", rows: [{ k: "File", v: "skills/daily.md" }], cost: "none", sponsor: "no chain transaction", sponsorColor: "var(--tx-3)", chainless: true };
function cer(head: CerSpec, phase: AppState["cerPhase"] = "review", extra = 0): AppState {
  const s = freshState("p1");
  s.queue = [head, ...Array.from({ length: extra }, () => head)];
  s.cerPhase = phase;
  return s;
}
const H = "0xaa5f2337da9c808fb2ad5e1f956315f065d21e8369c061776c3bb6933ce0a1c6";
function gate(failing: string[] = []): DeployGateRecord {
  return {
    initcodeHash: H,
    bindingHash: "0x" + "cd".repeat(32),
    compiler: { solcVersion: "0.8.28", optimizer: true, optimizerRuns: 200, evmVersion: "cancun", viaIr: false },
    verdict: failing.length ? "NOT_READY" : "READY",
    items: GATE_ITEM_IDS.map((id) => ({
      id,
      label: id,
      pass: !failing.includes(id),
      reason: failing.includes(id) ? "1 high finding" : "ok",
      evidence: { counts: { high: failing.includes(id) ? 1 : 0 }, outputSha256: "ab".repeat(32), durationMs: 1000, toolVersion: null },
    })),
    evaluatedAtMs: 1,
  };
}
const view: CeremonyView = { id: "cer-1", origin: "agent:hermes", kind: "transaction", chainId: 40204, decoded: { action: "Deploy contract (12 bytes)", destination: "contract creation", cost: "gas" }, requiresRawAck: false };
function review(over: Partial<NonNullable<AppState["walletReview"]>> = {}): AppState {
  const s = freshState("p1");
  s.walletReview = { kind: "deploy", label: "Deploy contract", view, rawAck: false, ...over } as NonNullable<AppState["walletReview"]>;
  return s;
}
const silent: BridgeTransport = { async send() {}, async listen() { return () => {}; } };
/** The deploy gate card sits inside the main window's <main>; standalone it gets one here. */
const inMain = (el: ReactElement) => charter(<main aria-label="Deploy gate card">{el}</main>);
const charter = (el: ReactElement) => (
  <div data-register="charter" style={{ minHeight: "100vh", background: "var(--srf-0)", color: "var(--tx-1)" }}>
    {el}
  </div>
);

const SCENES: Record<string, () => ReactElement> = {
  "monitor-idle": () => <ActivityMonitor snapshot={buildMonitorSnapshot(inputs)} now={62_000} onStop={noop} />,
  "monitor-running-plan-approvals": () => (
    <ActivityMonitor
      snapshot={buildMonitorSnapshot({
        ...inputs,
        providerKind: "sidecar",
        activity: running,
        daemons: daemonsSection({ allPaused: false, daemons: [daemon("a", "measured", true), daemon("b", null, false)] }, { blocked: null, error: null }),
      })}
      now={62_000}
      onStop={noop}
      onPauseDaemon={noop}
      onStopDaemon={noop}
    />
  ),
  "monitor-stopping": () => <ActivityMonitor snapshot={buildMonitorSnapshot({ ...inputs, activity: { ...running, state: "stopping" } })} now={62_000} onStop={noop} />,
  "monitor-unknowns": () => <ActivityMonitor snapshot={buildMonitorSnapshot({ ...inputs, tier: null, providerKind: "agent", providerLabel: "provider · x · agentic" })} now={62_000} onStop={noop} />,
  "popout-waiting": () => <PopoutRoot kind="monitor" transport={async () => silent} />,
  "popout-failed": () => (
    <PopoutRoot
      kind="monitor"
      transport={async () => {
        throw new Error("no ipc");
      }}
    />
  ),
  "popout-not-built": () => <PopoutRoot kind="diff" transport={async () => silent} />,
  "popout-browser-waiting": () => <PopoutRoot kind="browser" transport={async () => silent} />,
  "ceremony-diff": () => charter(<SignatureCeremony store={fakeStore()} s={cer({ ...base, card: diffCard("skill_write", write, "skills/daily.md", "keep\nold", "keep\nnew") })} />),
  "ceremony-command": () => charter(<SignatureCeremony store={fakeStore()} s={cer({ ...base, card: commandCard("run", write, ["rm", "-rf", "a b"], "/w") })} />),
  "ceremony-fields-hic": () =>
    charter(<SignatureCeremony store={fakeStore()} s={cer({ ...base, hic: { reason: "this session read untrusted content" }, card: fieldsCard("gsheets_append", write, { spreadsheetId: "1AbC", range: "Budget!A:C", rows: [["tea", 2]] }, "add 1 row to Budget!A:C") })} />),
  "ceremony-warning-queue": () => charter(<SignatureCeremony store={fakeStore()} s={cer({ ...base, warning: "This cannot be undone." }, "review", 2)} />),
  "ceremony-busy": () => charter(<SignatureCeremony store={fakeStore()} s={cer(base, "busy")} />),
  "ceremony-done": () => charter(<SignatureCeremony store={fakeStore()} s={cer(base, "done")} />),
  "review-chain-hic": () => charter(<WalletReviewModal store={fakeStore()} s={review({ card: chainCard("contract_deploy", { effect: "sign", trust: "trusted" }, view), hic: { reason: "tainted" } })} />),
  "review-raw": () => charter(<WalletReviewModal store={fakeStore()} s={review({ view: { ...view, requiresRawAck: true } })} />),
  "review-gate-not-ready": () => charter(<WalletReviewModal store={fakeStore()} s={review({ deployGate: gate(["slither"]) } as never)} />),
  "gate-ready": () => inMain(<DeployGateCard record={gate()} initcodeHash={H} />),
  "gate-not-ready": () => inMain(<DeployGateCard record={gate(["slither", "medusa"])} initcodeHash={H} />),
  "gate-none": () => inMain(<DeployGateCard record={null} initcodeHash={H} />),
};

(window as unknown as { __sceneIds: string[] }).__sceneIds = Object.keys(SCENES);
const id = decodeURIComponent(location.hash.slice(1));
const scene = SCENES[id];
const root = document.getElementById("root") as HTMLElement;
if (scene) {
  createRoot(root).render(<StrictMode>{scene()}</StrictMode>);
  // Mark ready after React has painted (pop-outs resolve their transport in a microtask).
  setTimeout(() => document.body.setAttribute("data-scene-ready", id), 300);
} else {
  root.textContent = "no scene " + id;
  document.body.setAttribute("data-scene-ready", "none");
}
