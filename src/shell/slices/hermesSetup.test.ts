// 0.5.4: the Hermes setup self-check. Core's report plus the app's own rows; a missing required
// item is counted and said, a failed read is an error row (never a pass).
import { describe, it, expect } from "vitest";
import { appItems, hermesSetupSlice, runHermesSelfCheck, setupNotice, type CoreSetupReport, type SelfCheckDeps } from "./hermesSetup";
import { annotatedAgentTools } from "../../agent/toolAnnotations";
import { AGENT_SYSTEM_PROMPT } from "../../agent/harness";

const coreOk: CoreSetupReport = {
  items: [
    { id: "hermes-binary", label: "Hermes sidecar", need: "required", state: "ok", detail: "bundled with the app" },
    { id: "embedding", label: "Memory search model", need: "optional", state: "missing", detail: "not found" },
  ],
  missingRequired: 0,
  firstRunDefaults: ["Hermes is offered the node's MCP read tools"],
};

function deps(over: Partial<SelfCheckDeps> = {}): SelfCheckDeps {
  return {
    core: async () => coreOk,
    status: async () => ({ running: true, skills: 6 }),
    tools: () => annotatedAgentTools(),
    prompt: () => AGENT_SYSTEM_PROMPT,
    now: () => 1,
    ...over,
  };
}

describe("the app's own rows", () => {
  it("this build's tools all carry their approval rule, and the prompt is present", async () => {
    const rows = await appItems(deps());
    expect(rows.find((r) => r.id === "agent-tools")?.state).toBe("ok");
    expect(rows.find((r) => r.id === "system-prompt")?.state).toBe("ok");
    expect(rows.find((r) => r.id === "skills-loaded")).toMatchObject({ state: "ok", detail: "Hermes reports 6 skills" });
  });

  it("a tool without an approval rule, or an empty prompt, is missing by name", async () => {
    const rows = await appItems(deps({ tools: () => [{ function: { name: "node_status" }, annotations: {} }, { function: { name: "new_tool" } }], prompt: () => "  " }));
    expect(rows.find((r) => r.id === "agent-tools")).toMatchObject({ state: "missing", detail: "no approval rule for new_tool" });
    expect(rows.find((r) => r.id === "system-prompt")?.state).toBe("missing");
  });

  it("skills are checked only while Hermes runs; a running Hermes with none is missing", async () => {
    expect((await appItems(deps({ status: async () => ({ running: false, skills: 0 }) }))).find((r) => r.id === "skills-loaded")?.state).toBe("off");
    expect((await appItems(deps({ status: async () => ({ running: true, skills: 0 }) }))).find((r) => r.id === "skills-loaded")?.state).toBe("missing");
    const failed = (await appItems(deps({ status: async () => Promise.reject(new Error("timed out")) }))).find((r) => r.id === "skills-loaded");
    expect(failed).toMatchObject({ state: "missing" });
    expect(failed?.detail).toContain("timed out");
  });
});

describe("runHermesSelfCheck", () => {
  it("joins core's rows and the app's, counts only required items, and fills the slice", async () => {
    const r = await runHermesSelfCheck(deps());
    expect(r.items?.map((i) => i.id)).toEqual(["hermes-binary", "embedding", "agent-tools", "system-prompt", "skills-loaded"]);
    expect(r.missingRequired).toBe(0);
    expect(setupNotice(r)).toBeNull();
    expect(hermesSetupSlice.get().firstRunDefaults).toEqual(["Hermes is offered the node's MCP read tools"]);
  });

  it("a missing required item is said at launch, by name", async () => {
    const r = await runHermesSelfCheck(
      deps({ core: async () => ({ ...coreOk, items: [{ id: "starter-capsules", label: "Starter skills (capsules)", need: "required", state: "missing", detail: "hello: not installed" }], missingRequired: 1 }) }),
    );
    expect(r.missingRequired).toBe(1);
    expect(setupNotice(r)).toBe("Hermes is missing one thing it needs: Starter skills (capsules). See Agent › Setup.");
  });

  it("a core report that cannot be read is an error, never a clean pass", async () => {
    const r = await runHermesSelfCheck(deps({ core: async () => Promise.reject(new Error("command hermes_setup_check not found")) }));
    expect(r.error).toContain("not found");
    expect(r.missingRequired).toBeGreaterThan(0);
    expect(setupNotice(r)).toContain("could not be checked");
  });
});
