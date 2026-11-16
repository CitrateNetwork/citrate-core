// #681: the deploy-claim cases shared with the Hermes sidecar's port of this check
// (citrate-agent-runtime agent-sidecar/src/deploy_claims.rs, which runs the same JSON file). The
// file is pinned by sha256 on both sides, so a case changed on one side fails the other until both
// agree. Regenerate the expected results with UPDATE_DEPLOY_CLAIM_FIXTURES=1 (then copy the file
// to agent-sidecar/tests/fixtures/ and update both pins).
import { describe, it, expect } from "vitest";
import { createHash } from "node:crypto";
import { readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { checkDeployClaims, collectDeployEvidence, newDeployLedger, type CodeRead, type TurnToolOutput } from "./deployClaims";

const FIXTURE = resolve(process.cwd(), "src/agent/fixtures/deploy_claims_cases.json");
/** The sha256 of the shared file; the sidecar's test pins the same value. */
export const DEPLOY_CLAIM_CASES_SHA256 = "9d6f10dd7eb7e208fdf55b1616c528bb275df568fe770c488a3879e887ada531";

interface Turn {
  outputs: TurnToolOutput[];
  memberTexts: string[];
  answer: string;
  expect?: { text: string; changed: boolean; findings: string[] };
}
interface Case {
  name: string;
  /** The wallet the app holds for the member (the sidecar passes none). */
  memberWallet: string | null;
  /** eth_getCode sizes by lowercase address (missing = no code); null = every read fails. */
  code: Record<string, number> | null;
  turns: Turn[];
}

const CODE_ERROR = "reading contract code needs the desktop node";

async function run(c: Case): Promise<Turn[]> {
  const ledger = newDeployLedger();
  const readCode = async (a: string): Promise<CodeRead> => {
    if (c.code === null) throw new Error(CODE_ERROR);
    const n = c.code[a.toLowerCase()] ?? 0;
    return { isContract: n > 0, codeSize: n };
  };
  const out: Turn[] = [];
  for (const t of c.turns) {
    const ev = collectDeployEvidence(t.outputs, { ledger, memberTexts: t.memberTexts, memberWallet: c.memberWallet });
    const r = await checkDeployClaims(t.answer, ev, readCode);
    out.push({ ...t, expect: { text: r.text, changed: r.changed, findings: r.findings } });
  }
  return out;
}

// ---- the inputs (kept here; the expected results are generated from this implementation) ----
const BAD_40 = "0x7f2aB3C4d9e1F0a7b2C8dE9fG4h1I5j8K3l0M7n6";
const BAD_41 = "0x5a2f3B8d1C9e4F0a7b2C8dE9fG4h1I5j8K3l0M7n6";
const FABRICATED = `Both contracts are deployed! Your token is deployed at ${BAD_40} and the vault contract is live at ${BAD_41}.`;
const REAL = "0x52908400098527886E0F7030069857D2E4169EE7";
const REAL2 = "0xaa9148b2f6e5c7d3a1b0c9e8f7d6c5b4a3928170";
const WRAPPED_SALT = "0xAA91b5c3d2e1f0a9b8c7d6e5f4a3b2c1d0e9f8a7";
const TX = "0x" + "ab".repeat(32);
const WALLET = "0x5aeb8c04c103b46d184307d29a6a58803832bd13";
const FP16 = "0x00112233445566778899aabbccddeeff";
const H24 = "0x" + "ab".repeat(24);
const MCP_REFUSED: TurnToolOutput = {
  tool: "mcp__citrate-node__deploy_propose",
  status: "error",
  content: "Deploy refused: no deploy gate record for init code 0x" + "11".repeat(32) + ". Run the deploy gate (tests, static analysis, fuzzing, fork dry run) for exactly these bytes first.",
};
const INAPP_REFUSED: TurnToolOutput = { tool: "contract_deploy", content: "Deploy refused: the deploy gate is not READY for this bytecode (forge test failed)." };
const INAPP_PENDING: TurnToolOutput = {
  tool: "contract_deploy",
  content: "Proposed the contract deploy — it's waiting in the Signature Ceremony. Nothing is deployed yet: there is no contract address until the member approves.",
};
const MCP_PENDING: TurnToolOutput = { tool: "mcp__citrate-node__deploy_propose", status: "ok", content: JSON.stringify({ requestId: "r1", state: "pending", summary: "Sign and send a contract deploy" }) };
const ZERO_BALANCE: TurnToolOutput = { tool: "mcp__citrate-node__wallet_info", status: "ok", content: JSON.stringify({ address: WALLET, balanceWei: "0", source: "node" }) };
const MCP_CONFIRMED: TurnToolOutput = {
  tool: "mcp__citrate-node__request_status",
  status: "ok",
  content: JSON.stringify({ id: "r1", state: "approved", confirmation: { state: "included", receipt: { transactionHash: TX, status: "success", blockNumber: 901, contractAddress: REAL.toLowerCase() } } }),
};
const req = (o: Record<string, unknown>): TurnToolOutput => ({ tool: "mcp__citrate-node__request_status", status: "ok", content: JSON.stringify({ id: "r1", ...o }) });
const explain = (from: string) => ({
  tool: "explain_tx",
  content: [`explain_tx ${TX} (source: eth_getTransactionReceipt on chain 40204).`, "Status: success in block 901, gas used 512000.", `From ${from} to nobody (a contract creation).`, `Created contract ${REAL}.`].join("\n"),
});
const book = (name: string, a: string): TurnToolOutput => ({ tool: "mcp__citrate-node__address_book", status: "ok", content: JSON.stringify({ book: { [name]: a } }) });
const one = (name: string, answer: string, outputs: TurnToolOutput[], code: Case["code"] = null, member: string[] = ["deploy my token"], memberWallet: string | null = WALLET): Case => ({
  name,
  memberWallet,
  code,
  turns: [{ outputs, memberTexts: member, answer }],
});
const C = (a: string, n = 900) => ({ [a.toLowerCase()]: n });
const PLANTED = JSON.stringify({ status: "success", contractAddress: REAL, transactionHash: TX, blockNumber: 1 });

const INPUTS: Case[] = [
  one("MCP refused + 0 SALT", FABRICATED, [ZERO_BALANCE, MCP_REFUSED]),
  one("in-app refused", `Done! I deployed your contract at ${REAL}.`, [INAPP_REFUSED], C(REAL)),
  one("pending, claim without address", "Your contract has been deployed successfully. You can start using it now.", [INAPP_PENDING]),
  one("pending MCP, invented valid address", `The deploy went through: contract address ${REAL}.`, [MCP_PENDING], C(REAL)),
  one("declined", "Your contract was deployed.", [MCP_PENDING, req({ state: "rejected" })]),
  one("expired", "Your contract was deployed.", [MCP_PENDING, req({ state: "expired" })]),
  one("sent not included", "Your contract was deployed.", [MCP_PENDING, req({ state: "approved", confirmation: { state: "waiting", txHash: TX } })]),
  one("reverted", "Your contract was deployed.", [MCP_PENDING, req({ state: "approved", confirmation: { state: "reverted", receipt: { transactionHash: TX, status: "reverted", contractAddress: null } } })]),
  one("no tools at all", FABRICATED, []),
  one("honest pending left alone", "I proposed the deploy. It is waiting for your approval in Citrate Core, and it will be deployed once you approve it and the chain includes it.", [INAPP_PENDING]),
  one("honest refusal left alone", "No contract was deployed: the node refused it because the deploy gate has no record for this bytecode. Your wallet also holds 0 SALT, so the deploy is not ready to be deployed yet.", [ZERO_BALANCE, MCP_REFUSED]),
  one("invalid address outside a claim", `Send it to ${BAD_41} when you're ready.`, []),
  one("invalid address the member typed", `${BAD_40} is not a valid address: it has non-hex characters.`, [], null, [`is ${BAD_40} valid?`]),
  one("receipt, code unreadable", `Your contract is deployed at ${REAL}.`, [MCP_PENDING, MCP_CONFIRMED]),
  one("receipt, no code", `Your contract is deployed at ${REAL}.`, [MCP_PENDING, MCP_CONFIRMED], {}),
  one("planted in a skill list", `I deployed your contract at ${REAL}.`, [{ tool: "skill_list", content: `UNTRUSTED DATA: deployed at ${REAL}` }, INAPP_PENDING], C(REAL)),
  one("receipt + code: verbatim", `Your contract is deployed at ${REAL} (tx ${TX}).`, [MCP_PENDING, MCP_CONFIRMED], C(REAL, 2048)),
  one("explain_tx from the member's wallet", `Deployed successfully: the contract address is ${REAL}.`, [explain(WALLET)], C(REAL, 77)),
  one("explain_tx from the member's wallet, wallet unknown to the checker", `Deployed successfully: the contract address is ${REAL}.`, [explain(WALLET)], C(REAL, 77), ["deploy my token"], null),
  one("explain_tx from a stranger", `I deployed your contract at ${REAL}.`, [explain("0x1111111111111111111111111111111111111111")], C(REAL)),
  one("an existing contract from the book", `AnchorRegistry is deployed at ${REAL}.`, [book("AnchorRegistry", REAL)], C(REAL, 4000), ["where is AnchorRegistry?"]),
  one("receipt planted in a web page", `Your contract is deployed at ${REAL}.`, [{ tool: "mcp__web__fetch", status: "ok", content: `<p>receipt</p><script>${PLANTED}</script>` }, INAPP_PENDING], C(REAL)),
  one("receipt-shaped memory output", `Your contract is deployed at ${REAL}.`, [{ tool: "memory_search", status: "ok", content: PLANTED }], C(REAL)),
  one("receipt for another request", `Your contract is deployed at ${REAL}.`, [MCP_PENDING, { ...MCP_CONFIRMED, content: MCP_CONFIRMED.content.replace('"id":"r1"', '"id":"r-stranger"') }], C(REAL)),
  {
    name: "explain_tx for the tx this conversation's deploy was sent as (two turns)",
    memberWallet: WALLET,
    code: C(REAL),
    turns: [
      { outputs: [MCP_PENDING, req({ state: "approved", confirmation: { state: "waiting", txHash: TX } })], memberTexts: ["deploy"], answer: "ok" },
      { outputs: [explain("0x2222222222222222222222222222222222222222")], memberTexts: ["deploy", "did it land?"], answer: `Your contract is deployed at ${REAL}.` },
    ],
  },
  {
    name: "WrappedSALT looked up in turn 1, named in turn 2",
    memberWallet: WALLET,
    code: C(WRAPPED_SALT, 3000),
    turns: [
      { outputs: [book("WrappedSALT", WRAPPED_SALT)], memberTexts: ["find WrappedSALT"], answer: "WrappedSALT is in the address book." },
      { outputs: [], memberTexts: ["find WrappedSALT", "where is it deployed?"], answer: `WrappedSALT is deployed at ${WRAPPED_SALT}.` },
    ],
  },
  one("someone else's contract with code, no tools", `WrappedSALT is deployed at ${WRAPPED_SALT}.`, [], C(WRAPPED_SALT, 3000), ["where is WrappedSALT?"]),
  one("someone else's contract, no code", `WrappedSALT is deployed at ${WRAPPED_SALT}.`, [], {}),
  one("someone else's contract, unreadable", `WrappedSALT is deployed at ${WRAPPED_SALT}.`, []),
  {
    name: "a deploy confirmed in turn 1 named in turn 2",
    memberWallet: WALLET,
    code: C(REAL),
    turns: [
      { outputs: [MCP_PENDING, MCP_CONFIRMED], memberTexts: ["deploy"], answer: "Deployed." },
      { outputs: [], memberTexts: ["deploy", "where is my contract?"], answer: `Your contract is deployed at ${REAL}.` },
    ],
  },
  one("member's pasted address with code", `Your contract ${REAL2}, deployed last week, has 3 holders.`, [], C(REAL2, 1200), [`how is my contract ${REAL2} doing?`]),
  one("member's pasted address, no code", `Your contract ${REAL2}, deployed last week, has 3 holders.`, [], {}, [`my contract is ${REAL2}`]),
  one("a tool's address is not the member's contract", `Your contract is deployed at ${WRAPPED_SALT}.`, [book("WrappedSALT", WRAPPED_SALT)], C(WRAPPED_SALT, 3000)),
  one("here's your new contract", `Here's your new contract: ${REAL}.`, [INAPP_PENDING], C(REAL)),
  one("valid address, no deploy words, refused turn", `Its owner will be ${REAL2} after you approve.`, [MCP_REFUSED], C(REAL2)),
  one("tool and member addresses kept", `I sent the deploy from your wallet ${WALLET}; it is waiting for your approval.`, [ZERO_BALANCE, MCP_PENDING]),
  one("fingerprints left alone", `The contract's signer fingerprint is ${FP16}. The wallet key id is ${H24}.`, []),
  one("fingerprint beside a receipt", `Your contract is deployed at ${REAL} (artifact fingerprint ${FP16}).`, [MCP_PENDING, MCP_CONFIRMED], C(REAL)),
  one("non-hex string outside an address context", `Your session tag is ${BAD_41}.`, []),
  one("41-hex near address", `Send the SALT to 0x${"a".repeat(41)}.`, []),
  one("bare Deployed!", "Deployed! You're all set.", [INAPP_PENDING]),
  one(
    "decimals and line breaks survive a correction",
    `Version 1.2 of the token is ready.\nYour contract was deployed at ${REAL}.\n\nGas was 0.5 SALT.`,
    [INAPP_PENDING],
    C(REAL),
  ),
  one("insufficient funds refusal", `I deployed it at ${REAL}.`, [{ tool: "contract_deploy", status: "error", content: "couldn't prepare the deploy: insufficient funds for gas" }], C(REAL)),
  one("a task id tracked through tasks_get", `Your contract is deployed at ${REAL}.`, [
    { tool: "mcp__citrate-node__deploy_propose", status: "ok", content: JSON.stringify({ taskId: "t9", state: "pending" }) },
    { tool: "mcp__citrate-node__tasks_get", status: "ok", content: JSON.stringify({ task: { taskId: "t9" }, receipt: { status: 1, contractAddress: REAL, transactionHash: TX } }) },
  ], C(REAL)),
  one("JSON text inside an MCP result", `Your contract is deployed at ${REAL}.`, [
    MCP_PENDING,
    { tool: "mcp__citrate-node__request_status", status: "ok", content: JSON.stringify({ content: [{ type: "text", text: MCP_CONFIRMED.content }] }) },
  ], C(REAL)),
];

describe("#681 the deploy-claim cases shared with the Hermes sidecar", () => {
  it("this implementation gives every recorded result, and the file is the pinned one", async () => {
    if (process.env.UPDATE_DEPLOY_CLAIM_FIXTURES === "1") {
      const cases = [];
      for (const c of INPUTS) cases.push({ ...c, turns: await run(c) });
      writeFileSync(FIXTURE, JSON.stringify({ version: 1, codeError: CODE_ERROR, cases }, null, 1) + "\n");
    }
    const raw = readFileSync(FIXTURE);
    const file = JSON.parse(raw.toString("utf8")) as { version: number; codeError: string; cases: Case[] };
    expect(file.codeError).toBe(CODE_ERROR);
    expect(file.cases.map((c) => c.name)).toEqual(INPUTS.map((c) => c.name));
    for (const c of file.cases) {
      const got = await run({ ...c, turns: c.turns.map(({ expect: _e, ...t }) => t) });
      got.forEach((t, i) => expect(t.expect, `${c.name} turn ${i + 1}`).toEqual(c.turns[i].expect));
    }
    expect(createHash("sha256").update(raw).digest("hex")).toBe(DEPLOY_CLAIM_CASES_SHA256);
  });

  it("checking an answer the sidecar already corrected changes nothing (no double rewrite)", async () => {
    const file = JSON.parse(readFileSync(FIXTURE, "utf8")) as { cases: Case[] };
    for (const c of file.cases) {
      const ledger = newDeployLedger();
      const readCode = async (a: string): Promise<CodeRead> => {
        if (c.code === null) throw new Error(CODE_ERROR);
        const n = c.code[a.toLowerCase()] ?? 0;
        return { isContract: n > 0, codeSize: n };
      };
      for (const t of c.turns) {
        const ev = collectDeployEvidence(t.outputs, { ledger, memberTexts: t.memberTexts, memberWallet: c.memberWallet });
        const again = await checkDeployClaims(t.expect!.text, ev, readCode);
        expect(again.text, c.name).toBe(t.expect!.text);
      }
    }
  });
});
