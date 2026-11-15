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
  store.setState({ chatMsgs: [], chatStatus: "ready" });
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
        tool: "mcp__citrate-node__deploy_propose",
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

  it("the pending in-app deploy: the corrected text is what the next turn sends back as context", async () => {
    store.provider = fakeProvider(async ({ callbacks }) => {
      callbacks.onActivity?.({
        kind: "tool_output",
        callId: "c2",
        tool: "contract_deploy",
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
        callId: "c3",
        tool: "mcp__citrate-node__request_status",
        status: "ok",
        content: JSON.stringify({
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
  });
});
