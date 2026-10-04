// HUP-S3.3 + S3.7 (core) — the rest of US-3.3 in the store:
//  - track workflows run from chat (`/run <workflow>` and the brief card), judged by the sidecar's
//    verifiers; only "verified" is reported as verified; without the sidecar loop the chat says
//    how to turn it on and runs nothing;
//  - the chosen persona travels with the sidecar session (skill allowlist + tool emphasis);
//  - "Read replies aloud" (off by default) speaks the answer with the persona's voice.
import { describe, it, expect, beforeEach, vi } from "vitest";
import { store } from "./store";
import { PERSIST_KEYS, freshState } from "./state";
import type { ChatProvider, TurnActivityEvent, WorkflowRunOpts } from "../agent/harness";
import type { WorkflowRunView } from "../agent/learn";
import type { HermesPersona, Brief } from "../bridge/domains";
import type { SpeechEngine, SpeechVoice } from "../agent/speech";

const GRAFT: HermesPersona = {
  id: "builder",
  role: "Builder",
  name: "Graft",
  name_status: "placeholder, pending owner sign-off",
  summary: "Ships code and dApps.",
  voice: "Direct.",
  tone: "Practical.",
  style_rules: ["Lead with the change."],
  default_track: "full-project",
  default_workflow: "hello-mint",
  tool_emphasis: ["forge_test"],
  skills: ["red-green"],
  tts_voice: "Daniel",
  prompt_fragment: "## Persona: Graft\n",
  name_pending_sign_off: true,
  custom: false,
};
const OWL: HermesPersona = { ...GRAFT, id: "custom-night-owl", name: "Night Owl", custom: true, name_pending_sign_off: false, skills: [], tool_emphasis: [], tts_voice: null };

function chatProvider(over: Partial<ChatProvider> = {}): ChatProvider {
  return {
    kind: "sidecar",
    label: "test",
    send: async ({ callbacks }) => {
      callbacks.onStatus("streaming");
      callbacks.onToken("**Hello** from Hermes");
      callbacks.onStatus("done");
      return { role: "assistant", content: "**Hello** from Hermes" };
    },
    ...over,
  };
}

function workflowProvider(view: WorkflowRunView, activity: TurnActivityEvent[] = []) {
  const runWorkflow = vi.fn(async (_id: string, opts: WorkflowRunOpts) => {
    for (const a of activity) opts.callbacks.onActivity?.(a);
    opts.callbacks.onStatus("done");
    return view;
  });
  store.provider = chatProvider({ runWorkflow });
  return runWorkflow;
}

const lastMsg = () => store.state.chatMsgs[store.state.chatMsgs.length - 1];

beforeEach(() => {
  store.setState({ ...freshState("p1"), chatMsgs: [], chatStatus: "ready" });
  store.speech = null;
});

describe("track workflows from chat", () => {
  it("/run <workflow> runs it through the provider and reports a verified run with its answers", async () => {
    const run = workflowProvider(
      { run_id: "wr-1", workflow_id: "status-note", state: "verified", answers: ["Done: a\nNext: b\nBlocked: none"] },
      [
        { kind: "verifier", step: "note", name: "journal_read succeeded", passed: true, detail: "" },
        { kind: "verifier", step: "note", name: "answer contains 'Next:'", passed: true, detail: "" },
      ],
    );
    await store.sendChat("/run status-note");
    expect(run).toHaveBeenCalledWith("status-note", expect.anything());
    const m = lastMsg();
    expect(m.who).toBe("Workflow");
    expect(m.text).toContain("status-note");
    expect(m.text).toContain("Verified");
    expect(m.text).toContain("Next: b");
    expect(m.chips.map((c) => c.status)).toEqual(["approved", "approved"]);
    expect(store.state.chatStatus).toBe("ready");
  });

  it("an unverified run is reported as not verified with the reason, and a failed check is marked", async () => {
    workflowProvider({ run_id: "wr-2", workflow_id: "project-plan", state: "unverified", reason: "step plan failed: journal_append was called" }, [
      { kind: "verifier", step: "plan", name: "journal_append not called", passed: false, detail: "it was called" },
    ]);
    await store.sendChat("/run project-plan");
    const m = lastMsg();
    expect(m.text).toContain("Not verified");
    expect(m.text).toContain("journal_append was called");
    expect(m.text).not.toMatch(/\bVerified\b/);
    expect(m.chips[0].status).toBe("declined");
  });

  it("a refused start says why and runs nothing", async () => {
    store.provider = chatProvider({
      runWorkflow: vi.fn(async () => {
        throw new Error("WORKFLOW_REFUSED: this workflow needs the contract toolchain (forge_test), which is off in this app");
      }),
    });
    await store.sendChat("/run contract-build");
    const m = lastMsg();
    expect(m.error ?? m.text).toContain("toolchain");
    expect(m.error ?? "").not.toContain("WORKFLOW_REFUSED");
    expect(store.state.chatStatus).toBe("ready");
  });

  it("without the sidecar loop the chat says how to turn it on and nothing runs", async () => {
    const send = vi.fn();
    store.provider = { kind: "local", label: "local", send } as unknown as ChatProvider;
    await store.sendChat("/run creative-project");
    expect(send).not.toHaveBeenCalled();
    const m = lastMsg();
    expect(m.text).toMatch(/sidecar/i);
    expect(m.text).toMatch(/Settings/);
  });

  it("/run with no workflow or a malformed id explains the command and runs nothing", async () => {
    const run = workflowProvider({ run_id: "wr-1", workflow_id: "x", state: "verified" });
    await store.sendChat("/run");
    expect(lastMsg().text).toMatch(/\/run <workflow>/);
    await store.sendChat("/run Not A Workflow!");
    expect(run).not.toHaveBeenCalled();
  });

  it("a saved brief's workflow runs from the brief card", async () => {
    const run = workflowProvider({ run_id: "wr-3", workflow_id: "creative-project", state: "verified", answers: ["Option 1"] });
    const brief = { track: "creative", workflow: "creative-project" } as unknown as Brief;
    await store.runBriefWorkflow(brief);
    expect(run).toHaveBeenCalledWith("creative-project", expect.anything());
  });

  it("ordinary messages still go to the model", async () => {
    const send = vi.fn(chatProvider().send);
    store.provider = chatProvider({ send });
    await store.sendChat("run the numbers for me");
    expect(send).toHaveBeenCalled();
  });
});

describe("the persona travels with the sidecar session", () => {
  it("none, a shipped persona by id, or a custom persona's own fields", () => {
    expect(store.sidecarPersonaChoice()).toBeNull();
    store.chooseHermesPersona(GRAFT);
    expect(store.sidecarPersonaChoice()).toEqual({ persona: "builder" });
    store.chooseHermesPersona(OWL);
    const c = store.sidecarPersonaChoice();
    expect(c && "customPersona" in c ? c.customPersona.id : null).toBe("custom-night-owl");
    expect(c && "customPersona" in c ? c.customPersona.style_rules : null).toEqual(OWL.style_rules);
  });
});

describe("read replies aloud", () => {
  const VOICES: SpeechVoice[] = [{ name: "Daniel", voiceURI: "u-daniel", lang: "en-GB", default: false }];
  function fakeSpeech(): SpeechEngine & { said: [string, SpeechVoice | null][] } {
    const said: [string, SpeechVoice | null][] = [];
    return { supported: true, voices: () => VOICES, speak: (t, v) => said.push([t, v]), cancel: () => undefined, said };
  }

  it("is off by default and persisted", () => {
    expect(freshState("p1").hermesReadAloud).toBe(false);
    expect(PERSIST_KEYS).toContain("hermesReadAloud");
  });

  it("off: nothing is spoken", async () => {
    const sp = fakeSpeech();
    store.speech = sp;
    store.provider = chatProvider();
    await store.sendChat("hello");
    expect(sp.said).toEqual([]);
  });

  it("on: the answer is spoken in the persona's voice, without markdown marks", async () => {
    const sp = fakeSpeech();
    store.speech = sp;
    store.setHermesReadAloud(true);
    store.chooseHermesPersona(GRAFT);
    store.provider = chatProvider();
    await store.sendChat("hello");
    expect(sp.said).toEqual([["Hello from Hermes", VOICES[0]]]);
  });

  it("on with no persona voice: the system voice speaks", async () => {
    const sp = fakeSpeech();
    store.speech = sp;
    store.setHermesReadAloud(true);
    store.provider = chatProvider();
    await store.sendChat("hello");
    expect(sp.said).toEqual([["Hello from Hermes", null]]);
  });

  it("a failed turn is not read aloud", async () => {
    const sp = fakeSpeech();
    store.speech = sp;
    store.setHermesReadAloud(true);
    store.provider = chatProvider({
      send: async () => {
        throw new Error("the model went away");
      },
    });
    await store.sendChat("hello");
    expect(sp.said).toEqual([]);
  });
});
