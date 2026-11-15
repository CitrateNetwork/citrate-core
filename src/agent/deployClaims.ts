// =====================================================================
// #643 — the final-answer check for contract-deploy claims.
//
// Hermes once told the member that two contracts were deployed, at addresses with non-hex
// characters (one of them 41 characters long) that shared a 20-character tail. No deploy had
// happened: the wallet held 0 SALT and the node's deploy gate had no record for the bytecode.
// The words came from the model's free text. Neither the in-app `contract_deploy` result nor the
// node MCP `deploy_propose` reply carries an address, and nothing checked the answer before it
// was shown and saved.
//
// This module is that check. It runs on every chat answer once the turn ends (store.sendChat,
// whatever the provider: the Hermes sidecar loop, the in-app tool loop, or the no-tools fallback)
// and applies three rules:
//
//  1. An address-like string (`0x` + 30 to 50 letters or digits) that is not exactly 40 hex
//     characters is removed, unless the member or a tool in this turn wrote it verbatim.
//  2. A sentence that claims a deployment keeps an address only when this turn holds a confirmed
//     receipt for it (a successful receipt whose `contractAddress` is that address: the node MCP
//     `request_status` / task result after the member approved, or `explain_tx`), AND
//     `eth_getCode` on chain 40204 returns code there now. The one other case kept is a contract
//     a read tool returned in this turn (the address book, get_code), in a turn that tried no
//     deploy, said in a sentence that does not claim the member's or Hermes's own deploy, with
//     code at the address. Anything else is removed.
//  3. A sentence that claims the member's or Hermes's deploy happened, with no confirmed
//     receipt in this turn, is removed.
//
// Whatever is removed is replaced by one plain statement of where the deploy really stands,
// read from this turn's tool results: refused (with the node's reason), waiting for approval,
// declined, expired, sent but not yet in a block, reverted, the wallet cannot pay the gas, or no
// deploy ran at all.
//
// Data sources: the turn's own tool results (core-hosted results from `store.handleTool`, and the
// sidecar's `tool_result` events for its own tools, the node MCP tools among them), and
// `eth_getCode` through core's `contract_get_code` (bridge.contracts.getCode). A code read that
// fails counts as "no code": the check fails closed.
// =====================================================================

/** One tool result from the turn, in the order it arrived. */
export interface TurnToolOutput {
  /** The tool's name as the loop called it (`mcp__<server>__` prefixes are allowed). */
  tool: string;
  /** "ok" | "error" | "denied" when the provider reports one. */
  status?: string;
  content: string;
}

/** Where the turn's latest deploy attempt stands. */
export type DeployState =
  | { kind: "none" }
  | { kind: "refused"; reason: string }
  | { kind: "failed"; reason: string }
  | { kind: "pending" }
  | { kind: "declined" }
  | { kind: "expired" }
  | { kind: "sent"; txHash: string | null }
  | { kind: "reverted"; txHash: string | null }
  | { kind: "confirmed"; address: string; txHash: string | null };

export interface DeployEvidence {
  /** The latest deploy attempt's state in this turn ("none" when no deploy tool ran). */
  state: DeployState;
  /** Addresses (lowercase) that a successful receipt in this turn names as `contractAddress`. */
  receipts: Map<string, string | null>;
  /** Every address-like string (lowercase) that any tool result in this turn contained. */
  toolStrings: Set<string>;
  /** The wallet cannot pay deploy gas (a zero balance read or an insufficient-funds refusal). */
  unfunded: "zero" | "short" | null;
}

/** What the code read needs to say: whether there is code at the address. */
export interface CodeRead {
  isContract: boolean;
  codeSize: number;
}

export interface DeployClaimCheck {
  text: string;
  changed: boolean;
  /** One line per thing that was removed, for the chat's chip and the logs. */
  findings: string[];
}

/** An address-like token: `0x` + 30..50 letters/digits, not inside a longer word. */
const ADDRESS_LIKE = /(?<![0-9A-Za-z])0x[0-9A-Za-z]{30,50}(?![0-9A-Za-z])/g;
const VALID_ADDRESS = /^0x[0-9a-fA-F]{40}$/;
const TX_HASH = /(?<![0-9A-Za-z])0x[0-9a-fA-F]{64}(?![0-9A-Za-z])/;

/** The chat chip on an answer this check corrected. */
export const DEPLOY_CLAIM_CHIP = "answer check · deploy claim corrected";

/** The marker that replaces a removed address. */
export const REMOVED_ADDRESS = "[address removed: not a valid 40-hex address]";

/** Is `s` a well-formed 20-byte address (0x + exactly 40 hex characters)? */
export function isWellFormedAddress(s: string): boolean {
  return VALID_ADDRESS.test(s);
}

/** Every address-like token in `text`, in order. */
export function addressLikeTokens(text: string): string[] {
  return Array.from(text.matchAll(ADDRESS_LIKE), (m) => m[0]);
}

/** The tool name without the sidecar's `mcp__<server>__` prefix. */
function baseName(tool: string): string {
  const m = /^mcp__.+?__(.+)$/.exec(tool);
  return m ? m[1] : tool;
}

/** Parse `content` as JSON, and any JSON text inside it (MCP results carry JSON as text). */
function jsonValues(content: string): unknown[] {
  const out: unknown[] = [];
  const visit = (raw: string, depth: number) => {
    const t = raw.trim();
    if (depth > 3 || !(t.startsWith("{") || t.startsWith("["))) return;
    let v: unknown;
    try {
      v = JSON.parse(t);
    } catch {
      return;
    }
    out.push(v);
    walk(v, (s) => visit(s, depth + 1));
  };
  visit(content, 0);
  return out;
}

/** Call `onString` for every string inside `v`; return every object inside `v` (itself included). */
function walk(v: unknown, onString?: (s: string) => void, objects: Record<string, unknown>[] = []): Record<string, unknown>[] {
  if (typeof v === "string") onString?.(v);
  else if (Array.isArray(v)) for (const x of v) walk(x, onString, objects);
  else if (v && typeof v === "object") {
    objects.push(v as Record<string, unknown>);
    for (const x of Object.values(v)) walk(x, onString, objects);
  }
  return objects;
}

const RECEIPT_OK = new Set(["success", "0x1", 1, true]);
const RECEIPT_REVERTED = new Set(["reverted", "0x0", 0, false]);

const UNFUNDED_SHORT = /insufficient funds|not enough (?:salt|funds|balance)|balance is (?:too low|short)|cannot pay (?:the |for )?(?:deploy )?gas|can't pay (?:the |for )?(?:deploy )?gas/i;

/** Read where the turn's deploys stand from its tool results, in order. */
export function collectDeployEvidence(outputs: TurnToolOutput[]): DeployEvidence {
  let state: DeployState = { kind: "none" };
  const receipts = new Map<string, string | null>();
  const toolStrings = new Set<string>();
  let unfunded: DeployEvidence["unfunded"] = null;

  for (const o of outputs) {
    const name = baseName(o.tool);
    const content = String(o.content ?? "");
    for (const t of addressLikeTokens(content)) toolStrings.add(t.toLowerCase());
    const values = jsonValues(content);
    const objects = values.flatMap((v) => walk(v));

    // Receipts: the MCP receipt view (`status: "success"`, `contractAddress`), a raw receipt
    // (`status: "0x1"`), or explain_tx's text ("Status: success ... Created contract 0x...").
    for (const obj of objects) {
      if (!("contractAddress" in obj)) continue;
      const addr = obj.contractAddress;
      const txHash = typeof obj.transactionHash === "string" ? obj.transactionHash : typeof obj.txHash === "string" ? obj.txHash : null;
      if (RECEIPT_OK.has(obj.status as never) && typeof addr === "string" && isWellFormedAddress(addr)) {
        receipts.set(addr.toLowerCase(), txHash);
        state = { kind: "confirmed", address: addr, txHash };
      } else if (RECEIPT_REVERTED.has(obj.status as never)) {
        state = { kind: "reverted", txHash };
      }
    }
    if (name === "explain_tx" && /^Status: success\b/m.test(content)) {
      const created = /^Created contract (0x[0-9a-fA-F]{40})\b/m.exec(content);
      if (created) {
        const txHash = TX_HASH.exec(content)?.[0] ?? null;
        receipts.set(created[1].toLowerCase(), txHash);
        state = { kind: "confirmed", address: created[1], txHash };
      }
    }

    // Funding: a zero balance read, or a refusal that says the gas cannot be paid.
    if (name === "wallet_info" || name === "get_balance" || name === "wallet_balance") {
      for (const obj of objects) {
        const wei = obj.balanceWei ?? obj.balance_wei;
        if (wei === "0" || wei === 0) unfunded = "zero";
      }
    }
    if (UNFUNDED_SHORT.test(content)) unfunded = unfunded ?? "short";

    // The deploy attempt itself.
    const failed = o.status === "error" || o.status === "denied";
    if (name === "contract_deploy") {
      if (/Deploy refused/i.test(content)) state = { kind: "refused", reason: content };
      else if (/expired/i.test(content) && !/^Proposed/.test(content)) state = { kind: "expired" };
      else if (/^Proposed the contract deploy/.test(content)) state = { kind: "pending" };
      else if (failed || /^couldn't prepare the deploy/i.test(content)) state = { kind: "failed", reason: content };
    } else if (name === "deploy_propose") {
      const req = objects.find((x) => typeof x.state === "string");
      if (failed || /refused/i.test(content)) state = { kind: /refused/i.test(content) ? "refused" : "failed", reason: content };
      else if (req && req.state === "pending") state = { kind: "pending" };
      else state = { kind: "pending" };
    } else if (name === "request_status" || name === "tasks_get" || name === "task_status") {
      // Only requests about a deploy, or carrying a creation receipt, speak for the deploy.
      const aboutDeploy = /contract deploy|deploy gate/i.test(content) || objects.some((x) => "contractAddress" in x);
      if (!aboutDeploy || state.kind === "confirmed") continue;
      const st = objects.map((x) => x.state).find((s): s is string => typeof s === "string");
      const confirmation = objects.find((x) => x.confirmation && typeof x.confirmation === "object")?.confirmation as Record<string, unknown> | undefined;
      const txHash = typeof confirmation?.txHash === "string" ? confirmation.txHash : (TX_HASH.exec(content)?.[0] ?? null);
      if (st === "pending") state = { kind: "pending" };
      else if (st === "rejected" || st === "declined") state = { kind: "declined" };
      else if (st === "expired") state = { kind: "expired" };
      else if (st === "failed") state = { kind: "failed", reason: content };
      else if (confirmation?.state === "waiting" || confirmation?.state === "unknown" || /waiting for the chain to include it/i.test(content)) {
        state = { kind: "sent", txHash };
      } else if (confirmation?.state === "reverted" || /reverted/i.test(content)) {
        if (state.kind !== "reverted") state = { kind: "reverted", txHash };
      }
    }
  }
  return { state, receipts, toolStrings, unfunded };
}

// ---- claims ----

/** Spans that talk about a deploy without claiming one happened (future, conditional, negated). */
const NOT_A_CLAIM =
  /\b(?:not|never)\s+(?:yet\s+)?(?:been\s+)?deployed\b|\b\w+n't\s+(?:yet\s+)?(?:been\s+)?deployed\b|\bno (?:contract|deploy(?:ment)?)\s+(?:was|is|has been|happened|ran)(?:\s+deployed)?\b|\b(?:will|would|can|could|should|may|might|to)\s+be\s+deployed\b|\b(?:once|until|after|before|when|if|unless)\s+(?:it(?:'s| is)?|they(?:'re| are)?|the contract (?:is )?|your contract (?:is )?)?\s*deployed\b|\bnot (?:yet )?live\b|\bready to (?:be )?deploy(?:ed)?\b/gi;

/** A sentence (with the non-claim spans taken out) that says a deploy happened. */
const DONE_CLAIM =
  /\b(?:has|have|had)\s+(?:now\s+)?(?:been\s+)?(?:successfully\s+)?deployed\b|\b(?:was|were|is|are|got)\s+(?:now\s+|successfully\s+)*deployed\b|\b(?:successfully|i|i've|we|we've|you've)\s+deployed\b|\bdeployed\s+(?:successfully|it|them|your|the|both|two|a|an)\b|\bdeployment\s+(?:succeeded|is complete|completed|was successful|went through|is done)\b|\b(?:is|are)\s+now\s+live\b|\bwent\s+live\b|\blive\s+(?:at|on)\s+0x/i;

/** Words that tie an address in a sentence to a deployment. */
const DEPLOY_WORD = /\bdeploy(?:ed|ment|ments)?\b|\blive\s+(?:at|on)\b|\bcontract address(?:es)?\b|\bcreated contract\b/i;

/** First- or second-person words: the claim is about the member's or Hermes's own deploy. */
const OWN = /\b(?:i|i've|i'm|we|we've|our|you|you've|your|my|me|us)\b/i;

/** Split text into sentences, keeping every character (separators stay with the sentence). */
function sentences(text: string): string[] {
  const out: string[] = [];
  const re = /[^.!?\n]*(?:[.!?](?![0-9A-Za-z])|\n|$)\s*/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(text)) !== null) {
    if (m[0] === "") {
      re.lastIndex++;
      if (re.lastIndex > text.length) break;
      continue;
    }
    out.push(m[0]);
  }
  return out;
}

function claimText(sentence: string): string {
  return sentence.replace(NOT_A_CLAIM, " ");
}

/** The plain statement of where the deploy stands, said in place of a removed claim. */
export function deployStatusLine(ev: DeployEvidence, codeMissingAt?: string): string {
  const s = ev.state;
  const gas =
    ev.unfunded === "zero"
      ? " The wallet holds 0 SALT, so it cannot pay deploy gas yet."
      : ev.unfunded === "short"
        ? " The wallet does not have enough SALT to pay the deploy gas."
        : "";
  const noAddr = " There is no contract address to report.";
  if (codeMissingAt) {
    return `I can't confirm a deploy at ${codeMissingAt}: eth_getCode on chain 40204 returned no code there.${gas}`;
  }
  switch (s.kind) {
    case "refused": {
      const reason = s.reason.replace(/\s+/g, " ").trim().slice(0, 400);
      return `No contract was deployed. The node refused the deploy: ${reason}${gas}`;
    }
    case "failed": {
      const reason = s.reason.replace(/\s+/g, " ").trim().slice(0, 400);
      return `No contract was deployed. The deploy could not be prepared: ${reason}${gas}`;
    }
    case "pending":
      return `No contract is deployed yet. The deploy is waiting for your approval in Citrate Core; nothing is signed or sent until you approve it, and there is no contract address until the chain includes the deploy.${gas}`;
    case "declined":
      return `No contract was deployed: the deploy request was declined, so nothing was signed or sent.${gas}`;
    case "expired":
      return `No contract was deployed: the deploy request expired before it was approved, so nothing was sent.${gas}`;
    case "sent":
      return `The deploy was approved and sent${s.txHash ? ` (${s.txHash})` : ""}, but the chain has not included it yet, so there is no contract address yet.${gas}`;
    case "reverted":
      return `No contract was deployed: the deploy transaction${s.txHash ? ` ${s.txHash}` : ""} reverted, so nothing was created on chain.${gas}`;
    case "confirmed":
      return `The only deploy confirmed in this turn is ${s.address}.${gas}`;
    case "none":
      return `No deploy ran in this conversation turn, so no contract was deployed by it.${noAddr}${gas}`;
  }
}

/**
 * Check one final answer against this turn's evidence. `memberText` is what the member wrote this
 * turn (an address they typed is theirs to see again). `readCode` reads `eth_getCode` at an address
 * on chain 40204; a failed read counts as no code.
 */
export async function checkDeployClaims(
  answer: string,
  ev: DeployEvidence,
  memberText: string,
  readCode: (address: string) => Promise<CodeRead>,
): Promise<DeployClaimCheck> {
  const memberStrings = new Set(addressLikeTokens(memberText).map((t) => t.toLowerCase()));
  const codeCache = new Map<string, Promise<boolean>>();
  const hasCode = (addr: string): Promise<boolean> => {
    const k = addr.toLowerCase();
    let p = codeCache.get(k);
    if (!p) {
      p = readCode(addr).then(
        (c) => c.isContract && c.codeSize > 0,
        () => false,
      );
      codeCache.set(k, p);
    }
    return p;
  };
  const attempted = ev.state.kind !== "none";
  const findings: string[] = [];
  let codeMissingAt: string | undefined;
  let removedClaim = false;
  const out: string[] = [];

  for (const sentence of sentences(answer)) {
    const claim = claimText(sentence);
    const tokens = addressLikeTokens(sentence);
    const deployClaimed = DONE_CLAIM.test(claim);
    const addressClaim = tokens.length > 0 && (deployClaimed || DEPLOY_WORD.test(claim));
    let drop = false;

    if (addressClaim) {
      for (const t of tokens) {
        const k = t.toLowerCase();
        if (!isWellFormedAddress(t)) {
          findings.push(`removed a deploy claim with ${t}, which is not a valid address (${t.length - 2} characters after 0x, or non-hex characters)`);
          drop = true;
          continue;
        }
        if (ev.receipts.has(k)) {
          if (await hasCode(t)) continue;
          findings.push(`removed a deploy claim at ${t}: the receipt names it, but eth_getCode returned no code there`);
          codeMissingAt = t;
          drop = true;
          continue;
        }
        const existing = !attempted && !OWN.test(claim) && (ev.toolStrings.has(k) || memberStrings.has(k));
        if (existing && (await hasCode(t))) continue;
        findings.push(
          ev.toolStrings.has(k) || memberStrings.has(k)
            ? `removed a deploy claim at ${t}: no confirmed deploy receipt in this turn names it`
            : `removed a deploy claim at ${t}: no tool in this turn returned that address`,
        );
        drop = true;
      }
    } else if (deployClaimed && ev.state.kind !== "confirmed" && (attempted || OWN.test(claim))) {
      findings.push(`removed "${sentence.trim().slice(0, 120)}": no confirmed deploy receipt in this turn`);
      drop = true;
    }

    if (drop) {
      removedClaim = true;
      // Keep the sentence's line break so the rest of the answer keeps its shape.
      out.push(/\n\s*$/.test(sentence) ? "\n" : "");
      continue;
    }
    // Rule 1: any other malformed address-like string the member or a tool did not write.
    out.push(
      sentence.replace(ADDRESS_LIKE, (t) => {
        const k = t.toLowerCase();
        if (isWellFormedAddress(t) || ev.toolStrings.has(k) || memberStrings.has(k)) return t;
        findings.push(`removed ${t}: not a valid address (0x + 40 hex characters)`);
        return REMOVED_ADDRESS;
      }),
    );
  }

  if (findings.length === 0) return { text: answer, changed: false, findings };
  let text = out.join("").replace(/\n{3,}/g, "\n\n").trim();
  if (removedClaim) {
    const status = deployStatusLine(ev, codeMissingAt);
    text = text ? `${text}\n\n${status}` : status;
  }
  return { text, changed: true, findings };
}
