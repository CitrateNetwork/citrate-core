// HGW-W3 (core #289): reading a `project_create` result (the runtime's JSON report) and knowing
// when its refusal means the projects folder is not set up (so the workspace card is offered).
import { describe, it, expect } from "vitest";
import { needsWorkspace, projectCreatedOf, PROJECT_CREATE_TOOL, WORKSPACE_HINT, WORKSPACE_HIC_REASON, WORKSPACE_TITLE } from "./projects";

const OK = JSON.stringify({
  ok: true,
  tool: "project_create",
  name: "demo",
  path: "/Users/m/Citrate/Projects/demo",
  template: null,
  marker: "/Users/m/Citrate/Projects/demo/.citrate/project.json",
  rendered: null,
  note: "This folder is a project",
});

describe("project_create results", () => {
  it("names the runtime's tool", () => {
    expect(PROJECT_CREATE_TOOL).toBe("project_create");
  });

  it("reads the created project from an ok result", () => {
    expect(projectCreatedOf("ok", OK)).toEqual({ name: "demo", path: "/Users/m/Citrate/Projects/demo" });
  });

  it("reads nothing from a refusal, an error or malformed text", () => {
    expect(projectCreatedOf("denied", OK)).toBeNull();
    expect(projectCreatedOf("error", OK)).toBeNull();
    expect(projectCreatedOf("ok", "not json")).toBeNull();
    expect(projectCreatedOf("ok", JSON.stringify({ ok: false, path: "/x", name: "x" }))).toBeNull();
    expect(projectCreatedOf("ok", JSON.stringify({ ok: true, path: "relative/x", name: "x" }))).toBeNull();
    expect(projectCreatedOf("ok", JSON.stringify({ ok: true, name: "x" }))).toBeNull();
  });

  it("offers the workspace card only for the missing or unshared projects folder", () => {
    expect(
      needsWorkspace("error", "tool error: the projects folder /Users/m/Citrate/Projects does not exist yet; the app creates it when the member lets Hermes work there"),
    ).toBe(true);
    expect(
      needsWorkspace("error", "tool error: the projects folder /Users/m/Citrate/Projects is not granted for reading and writing (no live Write grant); the member can allow it in the app"),
    ).toBe(true);
    expect(needsWorkspace("error", "tool error: /Users/m/Citrate/Projects/demo already exists; pick another name")).toBe(false);
    expect(needsWorkspace("denied", "declined: the member said no. Nothing was done.")).toBe(false);
    expect(needsWorkspace("ok", OK)).toBe(false);
  });

  it("uses plain copy with no em-dashes", () => {
    for (const s of [WORKSPACE_TITLE, WORKSPACE_HINT, WORKSPACE_HIC_REASON]) expect(s).not.toMatch(/—/);
  });
});
