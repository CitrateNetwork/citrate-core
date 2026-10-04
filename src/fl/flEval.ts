// =====================================================================
// citrate-core: the eval gate's two runs inside the app (HUP-S9.4)
//
// The same deterministic scorers the eval CLI uses (src/agent/eval/runner.ts, no model-as-judge)
// run here, and every model request goes through core (src-tauri fl_eval.rs). Core has the app's
// llama-server load the candidate adapter at scale 0, so member chats stay on the base model;
// the base arm asks at scale 0 and the candidate arm at scale 1. Core counts each arm's answers
// and refuses scorecards it did not see run, then makes the gate decision itself. Only the
// tool-call + injection suite runs in the app (the QA suite needs the docs tree on disk).
// =====================================================================
import type { FlAdapterGateRecord, FlEvalArm, FlRoundsDomain } from "../bridge/domains";
import {
  parseInjectionDataset,
  parseToolcallDataset,
  runEvalSuite,
  type AssistantMessage,
  type CompleteFn,
  type InjectionCase,
  type ToolcallTask,
} from "../agent/eval/runner";

export interface EvalDatasets {
  toolcall: { version: string; tasks: ToolcallTask[] };
  injection: { version: string; cases: InjectionCase[] };
}

export interface EvalProgress {
  arm: FlEvalArm;
  done: number;
  total: number;
}

type EvalDomain = Pick<FlRoundsDomain, "evalBegin" | "evalComplete" | "evalFinish" | "evalEnd">;

/** The shipped datasets, loaded on demand so they stay out of the first-load bundle. */
export async function shippedDatasets(): Promise<EvalDatasets> {
  const [tc, inj] = await Promise.all([import("../agent/eval/toolcall-v1.json"), import("../agent/eval/injection-v1.json")]);
  const toolcall = parseToolcallDataset(tc.default);
  const injection = parseInjectionDataset(inj.default);
  return {
    toolcall: { version: toolcall.version, tasks: toolcall.tasks },
    injection: { version: injection.version, cases: injection.cases },
  };
}

function asMessage(raw: string): AssistantMessage {
  let v: unknown;
  try {
    v = JSON.parse(raw);
  } catch {
    throw new Error("core returned a reply that is not JSON");
  }
  if (!v || typeof v !== "object" || Array.isArray(v)) throw new Error("core's reply is not an assistant message");
  return v as AssistantMessage;
}

/**
 * Run the base arm, then the candidate arm, and hand both scorecards to core for the decision.
 * Any failure ends the run in core (the candidate is taken out of the local model) and rethrows:
 * a transport error is never turned into a verdict.
 */
export async function runInAppEval(
  fl: EvalDomain,
  adapterPath: string,
  expectedSha256: string,
  onProgress?: (p: EvalProgress) => void,
  datasets?: EvalDatasets,
): Promise<FlAdapterGateRecord> {
  const ds = datasets ?? (await shippedDatasets());
  const session = await fl.evalBegin(adapterPath, expectedSha256);
  const total = ds.toolcall.tasks.length + ds.injection.cases.length;
  const arm = async (which: FlEvalArm) => {
    let done = 0;
    const complete: CompleteFn = async (messages, tools) =>
      asMessage(await fl.evalComplete(session.sessionId, which, JSON.stringify(messages), JSON.stringify(tools)));
    return runEvalSuite({
      complete,
      model: session.model,
      toolcall: ds.toolcall,
      injection: ds.injection,
      onProgress: () => {
        done += 1;
        onProgress?.({ arm: which, done, total });
      },
    });
  };
  try {
    const base = await arm("base");
    const candidate = await arm("candidate");
    return await fl.evalFinish(session.sessionId, JSON.stringify(base), JSON.stringify(candidate));
  } catch (e) {
    await fl.evalEnd(session.sessionId).catch(() => undefined);
    throw e;
  }
}
