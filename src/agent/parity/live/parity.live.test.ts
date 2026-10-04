// @vitest-environment node
// HUP-S1.9: LIVE parity, every parity-v1 scenario through core's sidecar provider, a REAL sidecar
// process and a scripted model over real HTTP. Runs only when CITRATE_PARITY_LIVE_SIDECAR names the
// sidecar binary (scripts/parity-live.sh passes the one inside the packaged .app); otherwise the
// suite is skipped and says why. Set CITRATE_PARITY_LIVE_REPORT to write the per-scenario report.
import { describe, it, expect, beforeAll, afterAll } from "vitest";
import { readFileSync, writeFileSync, existsSync } from "node:fs";
import { createHash, randomBytes } from "node:crypto";
import { resolve } from "node:path";
import { annotatedAgentTools } from "../../toolAnnotations";
import { AGENT_SYSTEM_PROMPT } from "../../harness";
import { ScriptedModel } from "../../../../scripts/parity-live/scripted-model.mjs";
import { startSidecar, HttpSessionApi } from "../../../../scripts/parity-live/sidecar-http.mjs";
import { compareLive, liveExpectation, liveStepCap, livePerStepCap, runLiveScenario, SESSION_CONFIG, SCRIPTED_ERROR_STATUS, type Fixture, type LiveResult, type RecordingSessionApi } from "./liveRunner";

const BIN = process.env.CITRATE_PARITY_LIVE_SIDECAR ?? "";
const CAPSULES = process.env.CITRATE_PARITY_LIVE_CAPSULES ?? "";
const REPORT = process.env.CITRATE_PARITY_LIVE_REPORT ?? "";
const RAW = readFileSync(resolve(process.cwd(), "src/agent/parity/parity-v1.json"));
const FIXTURE = JSON.parse(RAW.toString("utf8")) as Fixture;
// The pin itself is asserted by ../parity.test.ts; the report records which bytes this run used.
const FIXTURE_SHA256 = createHash("sha256").update(RAW).digest("hex");

const live = BIN !== "" && existsSync(BIN);

describe.skipIf(!live)("parity-v1 LIVE: sidecar provider → real sidecar → scripted model (HUP-S1.9)", () => {
  const model = new ScriptedModel({ errorStatus: SCRIPTED_ERROR_STATUS });
  let sidecar: { baseUrl: string; bearer: string; stderr: () => string; stop: () => Promise<void> };
  let modelBaseUrl = "";
  // The llm bearer core would pass from serve state; minted per run, never a literal.
  const llmBearer = randomBytes(16).toString("hex");
  const report: { id: string; layer: string; divergence: string | null; mismatches: string[]; outcome: string | null; model_calls: number; host_calls: string[] }[] = [];

  beforeAll(async () => {
    modelBaseUrl = await model.start();
    sidecar = await startSidecar(BIN, { capsulesDir: CAPSULES || undefined });
  }, 60_000);

  afterAll(async () => {
    await sidecar?.stop();
    await model.stop();
    if (REPORT) {
      const binSha = createHash("sha256").update(readFileSync(BIN)).digest("hex");
      writeFileSync(
        REPORT,
        JSON.stringify(
          {
            fixture: { version: FIXTURE.version, sha256: FIXTURE_SHA256 },
            sidecar: { path: BIN, sha256: binSha },
            session_config: SESSION_CONFIG,
            live_step_cap: liveStepCap(),
            live_per_step_cap: livePerStepCap(),
            harness_ts_turn_cap: FIXTURE.limits.ts_max_turns,
            scenarios: report,
          },
          null,
          2,
        ),
      );
    }
  });

  it("the shipped session config sets the step and per-reply caps the live run uses", () => {
    // default_turn_cap (owner decision): harness.ts stops at 6 model requests; a sidecar session
    // opened by core stops at this cap. Recorded, not changed here.
    expect(liveStepCap()).toBeGreaterThanOrEqual(FIXTURE.limits.rust_max_steps);
    expect(livePerStepCap()).toBe(FIXTURE.limits.rust_max_tool_calls_per_step);
  });

  for (const s of FIXTURE.scenarios) {
    it(`${s.id}${s.known_divergence?.sidecar ? " (known divergence: sidecar override)" : ""}`, async () => {
      const ex = liveExpectation(FIXTURE, s, liveStepCap());
      const r: LiveResult = await runLiveScenario(s, {
        model,
        makeApi: (build) => new HttpSessionApi(sidecar.baseUrl, sidecar.bearer, build) as RecordingSessionApi,
        modelBaseUrl,
        llmBearer,
        systemPrompt: AGENT_SYSTEM_PROMPT,
        tools: annotatedAgentTools(),
      });
      const mismatches = compareLive(ex, r, { maxToolsPerRequest: SESSION_CONFIG.maxToolsPerRequest, llmBearer });
      report.push({
        id: s.id,
        layer: s.layer,
        divergence: s.known_divergence?.verdict ?? null,
        mismatches,
        outcome: r.outcome,
        model_calls: r.requests.length,
        host_calls: r.hostCalls.map((c) => c.call.name),
      });
      expect(mismatches, `${s.id}\n${sidecar.stderr().slice(-1500)}`).toEqual([]);
    }, 60_000);
  }
});

describe.skipIf(live)("parity-v1 LIVE (skipped)", () => {
  it("needs CITRATE_PARITY_LIVE_SIDECAR=<sidecar binary>; run scripts/parity-live.sh against a packaged build", () => {
    expect(live).toBe(false);
  });
});
