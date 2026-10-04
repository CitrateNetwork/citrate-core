// HUP-S1.9 (live parity) — the chat header names what really answers. The sidecar loop is on by
// default (owner decision 2026-10-01) and runs on the local model, so its header is the local one,
// never the yellow "demo" dot, and its label no longer says "preview".
import { describe, it, expect } from "vitest";
import { store } from "./store";
import type { ChatProvider } from "../agent/harness";
import { createSidecarProvider, type SidecarSessionApi } from "../agent/sidecarProvider";

const unusedApi: SidecarSessionApi = {
  open: async () => "s",
  send: async () => undefined,
  events: async () => ({ events: [], lastSeq: 0, busy: false }),
  toolResult: async () => undefined,
  stop: async () => undefined,
};

function reflect(p: ChatProvider) {
  store.provider = p;
  (store as unknown as { reflectProvider(): void }).reflectProvider();
  return { kind: store.state.chatProviderKind, label: store.state.chatProviderLabel };
}

const stub = (kind: string): ChatProvider => ({ kind, label: kind, send: async () => ({ role: "assistant", content: "" }) }) as ChatProvider;

describe("chat header provider kind (HUP-S1.9)", () => {
  it("shows the sidecar loop as the local model, not the demo agent", () => {
    const r = reflect(createSidecarProvider(unusedApi, () => "p", () => []));
    expect(r.kind).toBe("local");
    expect(r.label).not.toMatch(/preview/i);
    expect(r.label).toMatch(/local model/);
  });

  it("keeps the other providers' kinds", () => {
    expect(reflect(stub("local")).kind).toBe("local");
    expect(reflect(stub("real")).kind).toBe("real");
    expect(reflect(stub("agent")).kind).toBe("real");
    expect(reflect(stub("demo")).kind).toBe("demo");
    expect(reflect(stub("something-new")).kind).toBe("demo");
  });
});
