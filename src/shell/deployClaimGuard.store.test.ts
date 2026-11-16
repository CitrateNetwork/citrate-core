// #643 — the chat's final answer goes through the deploy-claim check before it is kept, whatever
// the provider. The owner's report: the node refused the deploy and the wallet held 0 SALT, yet
// Hermes's reply said two contracts were deployed at invalid addresses.
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { store } from "./store";
import { bridge } from "../bridge";
import type { ChatProvider } from "../agent/harness";

const BAD_40 = "0x7f2aB3C4d9e1F0a7b2C8dE9fG4h1I5j8K3l0M7n6";
const BAD_41 = "0x5a2f3B8d1C9e4F0a7b2C8dE9fG4h1I5j8K3l0M7n6";
const REAL = "0x52908400098527886E0F7030069857D2E4169EE7";
const TX = "0x" + "cd".repeat(32);

const fakeProvider = (send: ChatProvider["send"]): ChatProvider =>
  ({ kind: "sidecar", label: "test", send }) as unknown as ChatProvider;

const asst = () => store.state.chatMsgs.find((m) => m.who !== "You");

beforeEach(() => {
  // A fresh conversation per test: each has its own deploy ledger.
  store.setState({ chatMsgs: [], chatStatus: "ready", chatThreadId: "t643-" + Math.random().toString(36).slice(2) });
});
afterEach(() => {
  vi.restoreAllMocks();
});

describe("#643 the chat answer is checked before it is kept", () => {
  it("a refused MCP deploy (sidecar tool) never stays as a deployed claim", async () => {
    store.provider = fakeProvider(async ({ callbacks }) => {
      callbacks.onActivity?.({
        kind: "tool_output",
        callId: "c1",
        tool: "mcp__node__deploy_propose",
        status: "error",
        content: "Deploy refused: no deploy gate record for this init code.",
      });
      callbacks.onStatus("streaming");
      callbacks.onToken(`Both contracts are deployed! Token: ${BAD_40}. `);
      callbacks.onToken(`Vault is live at ${BAD_41}.`);
      callbacks.onStatus("done");
    });
    await store.sendChat("deploy my token and vault");
    const m = asst();
    expect(m?.text).not.toContain(BAD_40);
    expect(m?.text).not.toContain(BAD_41);
    expect(m?.text).not.toMatch(/are deployed!/);
    expect(m?.text).toContain("No contract was deployed. The node refused the deploy: Deploy refused: no deploy gate record");
    expect(m?.chips.map((c) => c.label)).toContain("answer check · deploy claim corrected");
    expect(m?.streaming).toBe(false);
  });

  it("the pending deploy: the corrected text is what the next turn sends back as context", async () => {
    // #681: a sidecar-reported result counts only from core's node server (a sidecar-hosted
    // tool named contract_deploy is not core's).
    store.provider = fakeProvider(async ({ callbacks }) => {
      callbacks.onActivity?.({
        kind: "tool_output",
        callId: "c2",
        tool: "mcp__node__deploy_propose",
        status: "ok",
        content: "Proposed the contract deploy — it's waiting in the Signature Ceremony.",
      });
      callbacks.onToken("Your contract has been deployed successfully.");
    });
    await store.sendChat("deploy it");
    expect(asst()?.text).toContain("No contract is deployed yet. The deploy is waiting for your approval");
    const seen: string[] = [];
    store.provider = fakeProvider(async ({ messages }) => {
      seen.push(...messages.map((x) => x.content));
    });
    await store.sendChat("thanks");
    expect(seen.join("\n")).not.toContain("has been deployed successfully");
  });

  it("a confirmed receipt with code at the address is kept verbatim", async () => {
    const getCode = vi.spyOn(bridge.contracts, "getCode").mockResolvedValue({
      address: REAL,
      bookNames: [],
      isContract: true,
      codeSize: 2048,
      codeHash: "0x" + "00".repeat(32),
      proxy: null,
      verified: { status: "unverified", contractName: null, note: "" },
      note: "",
    } as never);
    const answer = `Your contract is deployed at ${REAL}.`;
    store.provider = fakeProvider(async ({ callbacks }) => {
      callbacks.onActivity?.({
        kind: "tool_output",
        callId: "c2",
        tool: "mcp__node__deploy_propose",
        status: "ok",
        content: JSON.stringify({ requestId: "r7", state: "pending" }),
      });
      callbacks.onActivity?.({
        kind: "tool_output",
        callId: "c3",
        tool: "mcp__node__request_status",
        status: "ok",
        content: JSON.stringify({
          id: "r7",
          state: "approved",
          summary: "Sign and send a contract deploy",
          confirmation: { state: "included", receipt: { transactionHash: TX, status: "success", blockNumber: 5, contractAddress: REAL } },
        }),
      });
      callbacks.onToken(answer);
    });
    await store.sendChat("did it deploy?");
    expect(asst()?.text).toBe(answer);
    expect(getCode).toHaveBeenCalledWith(REAL);
    expect(asst()?.chips.map((c) => c.label)).not.toContain("answer check · deploy claim corrected");
  });

  it("core-hosted tool results count as evidence (the in-app refusal)", async () => {
    vi.spyOn(bridge.contracts, "deploy").mockRejectedValue(new Error("Deploy refused: the deploy gate is not READY for this bytecode."));
    store.provider = fakeProvider(async ({ callbacks }) => {
      await callbacks.onToolCall({ id: "c4", name: "contract_deploy", arguments: { bytecodeHex: "0x6080" } } as never);
      callbacks.onToken(`Deployed! Contract address: ${REAL}.`);
    });
    await store.sendChat("deploy it");
    expect(asst()?.text).not.toContain(REAL);
    expect(asst()?.text).toContain("The node refused the deploy: Deploy refused: the deploy gate is not READY");
    expect(asst()?.text).not.toMatch(/^Deployed!/);
  });

  it("#680: turn 2 may say where a contract turn 1 looked up is deployed (code is there)", async () => {
    const WSALT = "0xAA91b5c3d2e1f0a9b8c7d6e5f4a3b2c1d0e9f8a7";
    vi.spyOn(bridge.contracts, "getCode").mockResolvedValue({ address: WSALT, bookNames: ["WrappedSALT"], isContract: true, codeSize: 3000, codeHash: null, proxy: null, verified: { status: "unverified", contractName: null, note: "" }, note: "" } as never);
    store.provider = fakeProvider(async ({ callbacks }) => {
      callbacks.onActivity?.({ kind: "tool_output", callId: "b1", tool: "mcp__node__address_book", status: "ok", content: JSON.stringify({ book: { WrappedSALT: WSALT } }) });
      callbacks.onToken("Found WrappedSALT in the address book.");
    });
    await store.sendChat("look up WrappedSALT");
    const answer = `WrappedSALT is deployed at ${WSALT}.`;
    store.provider = fakeProvider(async ({ callbacks }) => {
      callbacks.onToken(answer);
    });
    await store.sendChat("where is it deployed?");
    const last = store.state.chatMsgs.filter((m) => m.who !== "You").pop();
    expect(last?.text).toBe(answer);
  });

  it("#680: read-aloud speaks the checked answer, never the streamed claim", async () => {
    const spoken: string[] = [];
    const s = store as unknown as { readAloud: (t: string) => void };
    vi.spyOn(s, "readAloud").mockImplementation((t: string) => {
      spoken.push(t);
    });
    store.provider = fakeProvider(async ({ callbacks }) => {
      callbacks.onActivity?.({ kind: "tool_output", callId: "c1", tool: "mcp__node__deploy_propose", status: "error", content: "Deploy refused: no deploy gate record." });
      callbacks.onToken(`Deployed! Your token is deployed at ${BAD_40}.`);
    });
    await store.sendChat("deploy");
    expect(spoken).toHaveLength(1);
    expect(spoken[0]).not.toContain(BAD_40);
    expect(spoken[0]).toContain("The node refused the deploy");
  });
});
