// HUP-S3.3 + S3.7 — the chosen persona's voice reaches the model, and nothing else changes. With
// no persona (the default) the outgoing messages are exactly what they were before personas.
import { describe, it, expect, beforeEach } from "vitest";
import { store } from "./store";
import { PERSIST_KEYS, freshState } from "./state";
import type { ChatProvider } from "../agent/harness";
import type { HermesPersona } from "../bridge/domains";

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
  skills: ["solidity"],
  tts_voice: null,
  prompt_fragment: "## Persona: Graft\n- Lead with the change.\n",
  name_pending_sign_off: true,
  custom: false,
};
const OWL: HermesPersona = { ...GRAFT, id: "custom-night-owl", name: "Night Owl", custom: true, name_pending_sign_off: false };

const capture = () => {
  const seen: { role: string; content: string }[][] = [];
  store.provider = {
    kind: "local",
    label: "test",
    send: async ({ messages, callbacks }: Parameters<ChatProvider["send"]>[0]) => {
      seen.push(messages);
      callbacks.onStatus?.("done");
      return { role: "assistant", content: "" };
    },
  } as unknown as ChatProvider;
  return seen;
};

beforeEach(() => {
  store.setState({ ...freshState("p1"), chatMsgs: [], chatStatus: "ready" });
});

describe("store persona selection", () => {
  it("defaults to no persona and persists the choice and the custom set", () => {
    expect(freshState("p1").hermesPersona).toBeNull();
    expect(freshState("p1").customPersonas).toEqual([]);
    expect(PERSIST_KEYS).toContain("hermesPersona");
    expect(PERSIST_KEYS).toContain("customPersonas");
  });

  it("no persona: the outgoing messages carry no system message", async () => {
    const seen = capture();
    await store.sendChat("hello");
    expect(seen[0].some((m) => m.role === "system")).toBe(false);
  });

  it("a chosen persona rides as one system message at the head", async () => {
    store.chooseHermesPersona(GRAFT);
    const seen = capture();
    await store.sendChat("hello");
    expect(seen[0][0]).toEqual({ role: "system", content: GRAFT.prompt_fragment.trim() });
    expect(seen[0].filter((m) => m.role === "system")).toHaveLength(1);
    expect(seen[0][seen[0].length - 1]).toEqual({ role: "user", content: "hello" });
  });

  it("going back to the default voice removes it", async () => {
    store.chooseHermesPersona(GRAFT);
    store.chooseHermesPersona(null);
    const seen = capture();
    await store.sendChat("hello");
    expect(seen[0].some((m) => m.role === "system")).toBe(false);
  });

  it("custom personas are added once by id and removing the chosen one resets the voice", () => {
    store.addCustomPersona(OWL);
    store.addCustomPersona({ ...OWL, voice: "Updated." });
    expect(store.state.customPersonas).toHaveLength(1);
    expect(store.state.customPersonas[0].voice).toBe("Updated.");
    store.chooseHermesPersona(OWL);
    store.removeCustomPersona(OWL.id);
    expect(store.state.customPersonas).toHaveLength(0);
    expect(store.state.hermesPersona).toBeNull();
  });

  it("a shipped persona can never be stored as a custom one", () => {
    store.addCustomPersona(GRAFT);
    expect(store.state.customPersonas).toHaveLength(0);
  });
});
