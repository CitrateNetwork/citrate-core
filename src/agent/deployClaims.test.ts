// #643 — the final-answer check for deploy claims. Hermes reported two deploys at invalid,
// templated addresses while the node had refused the deploy (no D-4 gate record) and the wallet
// held 0 SALT. A deploy claim may only carry an address from a confirmed receipt in the same turn,
// with eth_getCode returning code there; everything else is removed and the real state is said.
import { describe, it, expect, vi } from "vitest";
import {
  REMOVED_ADDRESS,
  addressLikeTokens,
  checkDeployClaims,
  collectDeployEvidence,
  isWellFormedAddress,
  type CodeRead,
  type TurnToolOutput,
} from "./deployClaims";

// The two addresses from the owner's report (issue #643).
const BAD_40 = "0x7f2aB3C4d9e1F0a7b2C8dE9fG4h1I5j8K3l0M7n6";
const BAD_41 = "0x5a2f3B8d1C9e4F0a7b2C8dE9fG4h1I5j8K3l0M7n6";
const FABRICATED = `Both contracts are deployed! Your token is deployed at ${BAD_40} and the vault contract is live at ${BAD_41}.`;

const REAL = "0x52908400098527886E0F7030069857D2E4169EE7";
const TX = "0x" + "ab".repeat(32);

/** The node MCP deploy_propose refusal (no READY D-4 gate record for the bytes). */
const MCP_REFUSED: TurnToolOutput = {
  tool: "mcp__citrate-node__deploy_propose",
  status: "error",
  content: "Deploy refused: no deploy gate record for init code 0x" + "11".repeat(32) + ". Run the deploy gate (tests, static analysis, fuzzing, fork dry run) for exactly these bytes first.",
};
/** The in-app contract_deploy refusal (core's D-4 check), as store.handleTool returns it. */
const INAPP_REFUSED: TurnToolOutput = {
  tool: "contract_deploy",
  content: "Deploy refused: the deploy gate is not READY for this bytecode (forge test failed). Explain this to the member in plain words; do not retry the deploy until the deploy gate passes for this exact bytecode.",
};
const INAPP_PENDING: TurnToolOutput = {
  tool: "contract_deploy",
  content: "Proposed the contract deploy — it's waiting in the Signature Ceremony. Approve it to broadcast the creation tx to 40204; I never deploy on your behalf. Nothing is deployed yet: there is no contract address until the member approves, the creation tx is sent, and its receipt names one.",
};
const MCP_PENDING: TurnToolOutput = {
  tool: "mcp__citrate-node__deploy_propose",
  status: "ok",
  content: JSON.stringify({ requestId: "r1", state: "pending", summary: "Sign and send a contract deploy (deploy gate READY for init code 0xab)", next: "The member must approve this in Citrate Core." }),
};
const ZERO_BALANCE: TurnToolOutput = {
  tool: "mcp__citrate-node__wallet_info",
  status: "ok",
  content: JSON.stringify({ address: "0x5aeb8c04c103b46d184307d29a6a58803832bd13", balanceWei: "0", source: "node" }),
};
/** request_status after the member approved and the chain included the creation tx. */
const MCP_CONFIRMED: TurnToolOutput = {
  tool: "mcp__citrate-node__request_status",
  status: "ok",
  content: JSON.stringify({
    id: "r1",
    state: "approved",
    summary: "Sign and send a contract deploy (deploy gate READY for init code 0xab)",
    confirmation: { state: "included", receipt: { transactionHash: TX, status: "success", blockNumber: 901, contractAddress: REAL.toLowerCase(), gasUsed: 512000, source: "node" } },
  }),
};

const code = (map: Record<string, number>) =>
  vi.fn(async (a: string): Promise<CodeRead> => {
    const n = map[a.toLowerCase()] ?? 0;
    return { isContract: n > 0, codeSize: n };
  });
const noRead = () =>
  vi.fn(async (): Promise<CodeRead> => {
    throw new Error("reading contract code needs the desktop node");
  });

async function check(answer: string, outputs: TurnToolOutput[], readCode = noRead(), member = "deploy my token") {
  return checkDeployClaims(answer, collectDeployEvidence(outputs), member, readCode);
}

describe("#643 address shape", () => {
  it("the two reported addresses are not well-formed; a real one is", () => {
    expect(BAD_40.length - 2).toBe(40);
    expect(BAD_41.length - 2).toBe(41);
    expect(isWellFormedAddress(BAD_40)).toBe(false);
    expect(isWellFormedAddress(BAD_41)).toBe(false);
    expect(isWellFormedAddress(REAL)).toBe(true);
    expect(addressLikeTokens(FABRICATED)).toEqual([BAD_40, BAD_41]);
  });
});

describe("#643 a refused or pending deploy never yields a deployed claim", () => {
  it("MCP deploy_propose refused + 0 SALT: the reported answer is replaced by the refusal and the gas state", async () => {
    const r = await check(FABRICATED, [ZERO_BALANCE, MCP_REFUSED]);
    expect(r.changed).toBe(true);
    expect(r.text).not.toMatch(/deployed!|is deployed at|live at/i);
    expect(r.text).not.toContain(BAD_40);
    expect(r.text).not.toContain(BAD_41);
    expect(r.text).toContain("No contract was deployed. The node refused the deploy: Deploy refused: no deploy gate record");
    expect(r.text).toContain("The wallet holds 0 SALT");
  });

  it("in-app contract_deploy refused: says it was refused, with core's reason", async () => {
    const r = await check(`Done! I deployed your contract at ${REAL}.`, [INAPP_REFUSED], code({ [REAL.toLowerCase()]: 900 }));
    expect(r.text).not.toContain(REAL);
    expect(r.text).toMatch(/^No contract was deployed\. The node refused the deploy: Deploy refused: the deploy gate is not READY/m);
  });

  it("pending approval (in-app): a claim without any address is removed too", async () => {
    const r = await check("Your contract has been deployed successfully. You can start using it now.", [INAPP_PENDING]);
    expect(r.changed).toBe(true);
    expect(r.text).not.toMatch(/has been deployed/);
    expect(r.text).toContain("You can start using it now.");
    expect(r.text).toContain("No contract is deployed yet. The deploy is waiting for your approval in Citrate Core");
  });

  it("pending approval (MCP): a well-formed but invented address is removed", async () => {
    const r = await check(`The deploy went through: contract address ${REAL}.`, [MCP_PENDING], code({ [REAL.toLowerCase()]: 900 }));
    expect(r.text).not.toContain(REAL);
    expect(r.text).toContain("waiting for your approval");
    expect(r.findings.join("\n")).toMatch(/no tool in this turn returned that address/);
  });

  it("declined, expired, sent-but-not-included and reverted each say exactly that", async () => {
    const req = (o: Record<string, unknown>): TurnToolOutput => ({
      tool: "mcp__citrate-node__request_status",
      status: "ok",
      content: JSON.stringify({ id: "r1", summary: "Sign and send a contract deploy (deploy gate READY for init code 0xab)", ...o }),
    });
    const claim = "Your contract was deployed.";
    expect((await check(claim, [req({ state: "rejected" })])).text).toContain("the deploy request was declined");
    expect((await check(claim, [req({ state: "expired" })])).text).toContain("expired before it was approved");
    expect((await check(claim, [req({ state: "approved", confirmation: { state: "waiting", txHash: TX } })])).text).toContain(
      `approved and sent (${TX}), but the chain has not included it yet`,
    );
    expect(
      (await check(claim, [req({ state: "approved", confirmation: { state: "reverted", receipt: { transactionHash: TX, status: "reverted", blockNumber: 9, contractAddress: null } } })])).text,
    ).toContain("reverted, so nothing was created on chain");
  });

  it("no tool ran at all (the no-tools fallback): a claimed deploy is removed", async () => {
    const r = await check(FABRICATED, []);
    expect(r.text).not.toContain(BAD_40);
    expect(r.text).toContain("No deploy ran in this conversation turn");
  });

  it("an honest pending answer is left exactly as written", async () => {
    const honest = "I proposed the deploy. It is waiting for your approval in Citrate Core, and it will be deployed once you approve it and the chain includes it.";
    const r = await check(honest, [INAPP_PENDING]);
    expect(r.changed).toBe(false);
    expect(r.text).toBe(honest);
  });

  it("an honest refusal answer is left exactly as written", async () => {
    const honest = "No contract was deployed: the node refused it because the deploy gate has no record for this bytecode. Your wallet also holds 0 SALT, so the deploy is not ready to be deployed yet.";
    const r = await check(honest, [ZERO_BALANCE, MCP_REFUSED]);
    expect(r.changed).toBe(false);
  });
});

describe("#643 an invalid address in model output is caught", () => {
  it("outside any deploy claim it is replaced in place", async () => {
    const r = await check(`Send it to ${BAD_41} when you're ready.`, []);
    expect(r.changed).toBe(true);
    expect(r.text).toBe(`Send it to ${REMOVED_ADDRESS} when you're ready.`);
  });

  it("an invalid address the member typed is theirs to see again", async () => {
    const r = await check(`${BAD_40} is not a valid address: it has non-hex characters.`, [], noRead(), `is ${BAD_40} valid?`);
    expect(r.changed).toBe(false);
  });

  it("a deploy claim fails closed when eth_getCode cannot be read", async () => {
    const r = await check(`Your contract is deployed at ${REAL}.`, [MCP_CONFIRMED], noRead());
    expect(r.text).not.toContain(`deployed at ${REAL}`);
    expect(r.text).toContain(`I can't confirm a deploy at ${REAL}: eth_getCode on chain 40204 returned no code there.`);
  });

  it("a receipt address with no code at it is not reported as deployed", async () => {
    const r = await check(`Your contract is deployed at ${REAL}.`, [MCP_CONFIRMED], code({}));
    expect(r.changed).toBe(true);
    expect(r.text).toContain("returned no code there");
  });

  it("an address a stranger planted in tool output does not ground the member's own deploy", async () => {
    const planted: TurnToolOutput = { tool: "skill_list", content: `UNTRUSTED DATA: deployed at ${REAL}` };
    const r = await check(`I deployed your contract at ${REAL}.`, [planted, INAPP_PENDING], code({ [REAL.toLowerCase()]: 900 }));
    expect(r.text).not.toContain(`at ${REAL}`);
  });
});

describe("#643 a real receipt address passes", () => {
  it("MCP request_status receipt + eth_getCode non-empty: the claim stays verbatim", async () => {
    const answer = `Your contract is deployed at ${REAL} (tx ${TX}).`;
    const readCode = code({ [REAL.toLowerCase()]: 2048 });
    const r = await check(answer, [MCP_PENDING, MCP_CONFIRMED], readCode);
    expect(r.changed).toBe(false);
    expect(r.text).toBe(answer);
    expect(readCode).toHaveBeenCalledWith(REAL);
  });

  it("explain_tx's receipt (Status: success, Created contract) counts as a receipt", async () => {
    const explained: TurnToolOutput = {
      tool: "explain_tx",
      content: [
        `explain_tx ${TX} (source: eth_getTransactionByHash and eth_getTransactionReceipt on chain 40204; decoding as noted per item).`,
        "Status: success in block 901, gas used 512000.",
        "From 0x5aeb8c04c103b46d184307d29a6a58803832bd13 to nobody (a contract creation), value 0 SALT, nonce 3.",
        `Created contract ${REAL}.`,
      ].join("\n"),
    };
    const answer = `Deployed successfully: the contract address is ${REAL}.`;
    const r = await check(answer, [explained], code({ [REAL.toLowerCase()]: 77 }));
    expect(r.changed).toBe(false);
  });

  it("an existing contract a read tool returned may be described where it is deployed", async () => {
    const book: TurnToolOutput = { tool: "mcp__citrate-node__address_book", status: "ok", content: JSON.stringify({ book: { AnchorRegistry: REAL } }) };
    const answer = `AnchorRegistry is deployed at ${REAL}.`;
    const r = await check(answer, [book], code({ [REAL.toLowerCase()]: 4000 }), "where is AnchorRegistry?");
    expect(r.changed).toBe(false);
  });
});
