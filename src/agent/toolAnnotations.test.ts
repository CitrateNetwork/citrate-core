// HUP-S2.4 / A8 — every chat-agent tool carries the runtime's taint annotations (effect + trust).
// The sidecar (citrate-agent-runtime, HUP-S2.7) treats an unannotated tool as effectful and its
// output as untrusted; core keeps ONE reviewed annotation per tool so the downgrade is precise.
// Enumeration tripwire: a new tool without an annotation, or an effectful tool marked `none`, fails.
import { describe, it, expect, vi, afterEach } from "vitest";
import { AGENT_TOOLS, READ_ONLY_AGENT_TOOLS, type ToolCall } from "./harness";
import { AGENT_TOOL_ANNOTATIONS, annotatedAgentTools, annotationFor } from "./toolAnnotations";
import { store } from "../shell/store";
import { bridge } from "../bridge";

const names = AGENT_TOOLS.map((t) => t.function.name as string);
const EFFECTS = ["none", "write", "spend", "sign"];
const TRUSTS = ["trusted", "untrusted"];

afterEach(() => vi.restoreAllMocks());

describe("Feature: every agent tool is annotated (A8)", () => {
  it("Given AGENT_TOOLS, then every entry has a valid effect and trust, and no annotation names a missing tool", () => {
    for (const n of names) {
      const a = annotationFor(n);
      expect(a, `tool ${n} has no annotation`).not.toBeNull();
      expect(EFFECTS).toContain(a!.effect);
      expect(TRUSTS).toContain(a!.trust);
    }
    for (const k of Object.keys(AGENT_TOOL_ANNOTATIONS)) expect(names).toContain(k);
    expect(annotationFor("no_such_tool")).toBeNull();
  });

  it("Given the reviewed read-only list, then a tool is `effect: none` exactly when it is on it", () => {
    for (const n of names) {
      expect(annotationFor(n)!.effect === "none", `tool ${n}`).toBe(READ_ONLY_AGENT_TOOLS.has(n));
    }
  });

  it("Given a tool that stops at a member approval when invoked, then it is never annotated `none` (and a `none` tool never asks)", async () => {
    const call = (name: string): ToolCall => ({
      id: "c1",
      name,
      arguments: JSON.stringify({ group: "g", name: "n", bytecodeHex: "0x00", fact: "f", entry: "e", instructions: "i", query: "q" }),
    });
    for (const n of names) {
      vi.restoreAllMocks();
      const sig = vi.spyOn(store, "requestSig").mockResolvedValue("declined");
      const review = vi.spyOn(store, "openWalletReview").mockImplementation(() => {});
      vi.spyOn(store, "go").mockImplementation(() => {});
      vi.spyOn(bridge.agentSkills, "write").mockRejectedValue(new Error("SKILL_EXISTS: exists"));
      vi.spyOn(bridge.agentSkills, "read").mockResolvedValue("old");
      vi.spyOn(bridge.contracts, "deploy").mockResolvedValue({ id: "cer1", origin: "o", kind: "transaction", chainId: 40204, decoded: { action: "a", cost: "c", destination: "d" }, requiresRawAck: false } as never);
      await store.handleTool(call(n), "m1", () => {});
      const gated = sig.mock.calls.length + review.mock.calls.length > 0;
      if (gated) expect(annotationFor(n)!.effect, `tool ${n} asks the member, so it has an effect`).not.toBe("none");
      else expect(annotationFor(n)!.effect, `tool ${n} never asks, so it must be read-only`).toBe("none");
    }
  });

  it("Given the sidecar session, then the annotated tool list carries {effect, trust} beside each OpenAI wrapper", () => {
    const tools = annotatedAgentTools();
    expect(tools).toHaveLength(AGENT_TOOLS.length);
    for (const t of tools) {
      expect(t.type).toBe("function");
      expect(t.annotations).toEqual(AGENT_TOOL_ANNOTATIONS[t.function.name]);
    }
    // the gateway / local tool loops keep sending the plain OpenAI shapes
    expect(JSON.stringify(AGENT_TOOLS)).not.toContain("annotations");
  });

  it("Given content written by people other than the member, then the tools that return it are untrusted", () => {
    for (const n of ["skills_list", "models_list", "directory_find", "groups_list", "group_roster", "skill_run"]) {
      expect(annotationFor(n)!.trust, n).toBe("untrusted");
    }
    expect(annotationFor("contract_deploy")!.effect).toBe("sign");
  });
});
