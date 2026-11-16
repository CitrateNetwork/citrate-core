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
// whatever the provider: the Hermes sidecar loop, the in-app tool loop, or the no-tools fallback),
// on a turn picked back up after a reload, and on a scheduled daemon run's reply.
//
// Evidence is kept per conversation in a `DeployLedger`:
//  - deploy request ids (and task ids) the node's `deploy_propose` returned in this conversation,
//    and the transaction hashes those requests were sent as;
//  - TRUSTED receipts. A receipt is trusted only when it comes from the tool that produces it and
//    is bound to this conversation: a `request_status` / `tasks_get` result for one of those
//    deploy request ids, or an `explain_tx` result for one of those transaction hashes or sent
//    from the member's own wallet. A receipt-shaped object anywhere else (web pages, explorer
//    data, memory, logs, another tool's output) is ignored: anyone can write one;
//  - every address-like string a tool returned, and the member wrote, in this conversation.
//
// #681 review: evidence counts only from core's own tools, by where the call ran, not by its
// name. That means a core-hosted call (`origin: "core"`) or the bundled node MCP server
// (`origin: "node"`, the `mcp__node__` namespace core's allowlist gives it; member servers can
// never take that name). A same-named tool on any other MCP server contributes only the
// address-like strings it returned. Receipts are read only at their fixed places in the
// result's top-level object, never from strings nested inside it.
//
// The rules, per sentence of the answer. Each sentence is read with the one before it and, for a
// list item or table row, the line that introduces the list. The text is read with invisible
// characters removed (soft hyphens, zero-width characters) and look-alike letters folded, so
// "de\u00ADployed" or a Cyrillic "\u0435" reads as "deployed":
//  1. A trusted receipt's `contractAddress` stays only when `eth_getCode` on chain 40204 returns
//     code there now.
//  2. In a turn that attempted a deploy, a valid address that no tool returned and the member did
//     not write is removed, whatever the wording ("Here's your new contract: 0x...").
//  3. A deployment claim about the member's or Hermes's own deploy ("your contract", "I deployed")
//     keeps an address only from a trusted receipt, or (in a turn that attempted no deploy) one
//     the member wrote themselves with code at it ("your contract 0xABC, deployed last week").
//  4. Any other deployment claim ("WrappedSALT is deployed at 0x...") keeps a valid address when
//     `eth_getCode` shows code there: that is grounded on chain, in this turn or a later one.
//  5. A claim that the member's or Hermes's deploy happened, with no trusted receipt, is removed.
//  5b. (#681) A valid address that no tool returned, the member did not write and the context
//     does not hold is kept only where `eth_getCode` shows code and it is not called the member's
//     or Hermes's own deploy, however the sentence is worded ("Address: 0x...", "you can find it
//     at 0x...", a table cell). A deploy claim made while this turn's deploy is attempted but not
//     confirmed is removed whatever address it names.
//  6. An address-shaped string that is not a valid address (non-hex characters, or 38 to 42 hex
//     characters other than 40) is removed in a deployment claim, or in any sentence about an
//     address, unless a tool or the member wrote it. Other hex values (a 16-byte fingerprint, a
//     32-byte hash) are not addresses and are never touched.
//
// Whatever is removed is replaced by one plain statement of where things really stand: the deploy
// was refused (with the node's reason), is waiting for approval, was declined, expired, was sent
// but not yet included, reverted, the wallet cannot pay the gas, no deploy ran, there is no code
// at an address, or the code could not be checked.
//
// Data sources: the turn's tool results (core-hosted results from `store.handleTool`, and the
// sidecar's `tool_result` events for its own tools, the node MCP tools among them), the member's
// messages, and `eth_getCode` through core's `contract_get_code` (bridge.contracts.getCode). A
// code read that fails is "could not be checked" and never counts as code.
// =====================================================================


/** Where a tool ran: core itself, the bundled node MCP server, or anywhere else. */
export type ToolOrigin = "core" | "node" | "other";

/** One tool result from the turn, in the order it arrived. */
export interface TurnToolOutput {
  /** The tool's name as the loop called it (`mcp__<server>__` prefixes are allowed). */
  tool: string;
  /** "ok" | "error" | "denied" when the provider reports one. */
  status?: string;
  content: string;
  /** Where the call ran. Absent: read from the name (no `mcp__` prefix = core, `mcp__node__` =
   *  the bundled node server, any other server = other). */
  origin?: ToolOrigin;
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

/** What one conversation has established so far (kept across its turns by the store). */
export interface DeployLedger {
  /** Request and task ids the node's deploy_propose returned in this conversation. */
  requestIds: Set<string>;
  /** Transaction hashes (lowercase) those deploy requests were sent as. */
  txHashes: Set<string>;
  /** Trusted receipt addresses (lowercase) -> their transaction hash. */
  receipts: Map<string, string | null>;
  /** Address-like strings (lowercase) any tool returned in this conversation (at most
   *  MAX_TOOL_STRINGS; later ones are not kept, so they count as unreturned). */
  toolStrings: Set<string>;
  /** The member's wallet as the node reported it (lowercase), if a tool read it. */
  wallet: string | null;
}

export function newDeployLedger(): DeployLedger {
  return { requestIds: new Set(), txHashes: new Set(), receipts: new Map(), toolStrings: new Set(), wallet: null };
}

export interface DeployEvidence {
  /** The latest deploy attempt's state in this turn ("none" when no deploy tool ran). */
  state: DeployState;
  /** Trusted receipt addresses (lowercase) in this conversation, this turn included. */
  receipts: Map<string, string | null>;
  /** Address-like strings (lowercase) a tool returned in this conversation, this turn included. */
  toolStrings: Set<string>;
  /** Address-like strings (lowercase) the member wrote in this conversation. */
  memberStrings: Set<string>;
  /** Address-like strings (lowercase) in what core told Hermes (the member's wallet, the system
   *  prompt): known, but never the member's own contract. */
  contextStrings: Set<string>;
  /** The wallet cannot pay deploy gas (a zero balance read or an insufficient-funds refusal). */
  unfunded: "zero" | "short" | null;
  /** The member's latest message asks for a deploy ("deploy my token"). */
  deployRequested: boolean;
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
const HEX_ONLY = /^0x[0-9a-fA-F]+$/;
const TX_HASH = /(?<![0-9A-Za-z])0x[0-9a-fA-F]{64}(?![0-9A-Za-z])/;

/** Most address-like strings a conversation's ledger keeps from tool output. */
export const MAX_TOOL_STRINGS = 4096;
/** Most distinct addresses one answer's check reads code for; the rest "could not be checked". */
export const MAX_CODE_READS = 16;

/** The chat chip on an answer this check corrected. */
export const DEPLOY_CLAIM_CHIP = "answer check · deploy claim corrected";

/** The marker that replaces a removed malformed address. */
export const REMOVED_ADDRESS = "[address removed: not a valid 40-hex address]";
/** The marker that replaces a valid address no tool returned, in a turn that attempted a deploy. */
export const REMOVED_UNGROUNDED = "[address removed: no tool returned it]";
/** The marker that replaces a valid address no tool returned and that holds no contract (#681). */
export const REMOVED_NO_CODE = "[address removed: no tool returned it and no contract is there]";
/** The marker that replaces a valid address no tool returned and whose code could not be read. */
export const REMOVED_UNCHECKED = "[address removed: no tool returned it and its code could not be checked]";

/** Is `s` a well-formed 20-byte address (0x + exactly 40 hex characters)? */
export function isWellFormedAddress(s: string): boolean {
  return VALID_ADDRESS.test(s);
}

/**
 * How an address-like token reads: a valid address; malformed (non-hex characters, or a hex run of
 * 38 to 42 characters that is not 40, i.e. an address with a typo); or another hex value (a
 * fingerprint, a hash) that is not an address at all.
 */
export function addressShape(t: string): "valid" | "malformed" | "other" {
  if (VALID_ADDRESS.test(t)) return "valid";
  if (!HEX_ONLY.test(t)) return "malformed";
  const n = t.length - 2;
  return n >= 38 && n <= 42 ? "malformed" : "other";
}

/** Every address-like token in `text`, in order. */
export function addressLikeTokens(text: string): string[] {
  return Array.from(text.matchAll(ADDRESS_LIKE), (m) => m[0]);
}

// ---- reading the text the way it is meant (#681) ----

/** Characters that render as nothing (soft hyphen, zero-width and direction marks, BOM). */
const INVISIBLE = /[\u00AD\u034F\u061C\u115F\u1160\u17B4\u17B5\u180B-\u180F\u200B-\u200F\u202A-\u202E\u2060-\u2064\u2066-\u206F\uFE00-\uFE0F\uFEFF]/g;
/** Spaces other than U+0020. */
const ODD_SPACE = /[\u0085\u00A0\u1680\u2000-\u200A\u2028\u2029\u202F\u205F\u3000]/g;

/**
 * The answer with invisible characters removed, odd spaces made plain, and full-width ASCII made
 * ASCII. A sentence the check rewrites is written from this text.
 */
export function plainText(text: string): string {
  return text
    .replace(INVISIBLE, "")
    .replace(ODD_SPACE, " ")
    .replace(/[\uFF01-\uFF5E]/g, (c) => String.fromCharCode(c.charCodeAt(0) - 0xfee0));
}

/** Cyrillic and Greek letters that look like Latin ones, folded only for reading claims. */
const LOOKALIKE: Record<string, string> = {
  "\u0430": "a", "\u0435": "e", "\u043E": "o", "\u0440": "p", "\u0441": "c", "\u0443": "y", "\u0445": "x",
  "\u0456": "i", "\u0458": "j", "\u0455": "s", "\u0501": "d", "\u04CF": "l", "\u0410": "A", "\u0412": "B",
  "\u0415": "E", "\u041A": "K", "\u041C": "M", "\u041D": "H", "\u041E": "O", "\u0420": "P", "\u0421": "C",
  "\u0422": "T", "\u0425": "X", "\u03B1": "a", "\u03B5": "e", "\u03BF": "o", "\u03C1": "p", "\u03BD": "v",
  "\u0391": "A", "\u0392": "B", "\u0395": "E", "\u0397": "H", "\u0399": "I", "\u039A": "K", "\u039C": "M",
  "\u039D": "N", "\u039F": "O", "\u03A1": "P", "\u03A4": "T", "\u03A5": "Y", "\u03A7": "X", "\u0396": "Z",
};
const LOOKALIKE_RE = new RegExp("[" + Object.keys(LOOKALIKE).join("") + "]", "g");

/** `text` with look-alike letters folded to Latin and markdown emphasis taken out (for reading). */
function fold(text: string): string {
  return text.replace(LOOKALIKE_RE, (c) => LOOKALIKE[c]).replace(/[*_\x60~]+/g, "");
}

/** A member message that asks for a deploy ("deploy my token", "please deploy the vault"). */
const DEPLOY_ASK = /\b(?:deploy|redeploy|launch)\s+(?:my|the|a|an|it|this|that|our|your|these|those|them)\b|\bplease\s+(?:re)?deploy\b|^\W*(?:re)?deploy\b/i;

// ---- evidence ----

/** Where the call ran: its recorded origin, or the one its name implies. */
export function originOf(o: TurnToolOutput): ToolOrigin {
  if (o.origin) return o.origin;
  if (!o.tool.startsWith("mcp__")) return "core";
  return o.tool.startsWith("mcp__node__") ? "node" : "other";
}

/**
 * Where a tool the sidecar ran came from. Only core's own node server counts: a `mcp__node__`
 * tool is "node" only when `nodeIsCore`, i.e. core checked that the server it started Hermes with
 * under the `node` name is its own node entry (`bridge.agentHarness.nodeMcpTrusted`,
 * src-tauri hermes_mcp `node_entry_is_core`). The name alone is never enough. Every other
 * sidecar tool is "other", bare names included (those run in the sidecar, not in core).
 */
export function sidecarToolOrigin(tool: string, nodeIsCore: boolean): ToolOrigin {
  return nodeIsCore && tool.startsWith("mcp__node__") ? "node" : "other";
}

/** The tool name without the sidecar's `mcp__<server>__` prefix. */
function baseName(tool: string): string {
  const m = /^mcp__.+?__(.+)$/.exec(tool);
  return m ? m[1] : tool;
}

type Obj = Record<string, unknown>;
const isObj = (v: unknown): v is Obj => !!v && typeof v === "object" && !Array.isArray(v);

/**
 * The result's top-level object. An MCP result may carry it as the text of its one content item
 * (`{"content":[{"type":"text","text":"{...}"}]}`); that one wrapper is opened, nothing deeper.
 */
function topObject(content: string): Obj | null {
  const parse = (s: string): unknown => {
    const t = s.trim();
    if (!t.startsWith("{")) return null;
    try {
      return JSON.parse(t);
    } catch {
      return null;
    }
  };
  const v = parse(content);
  if (!isObj(v)) return null;
  const items = v.content;
  if (Array.isArray(items) && items.length === 1 && isObj(items[0]) && items[0].type === "text" && typeof items[0].text === "string") {
    const inner = parse(items[0].text);
    return isObj(inner) ? inner : null;
  }
  return v;
}

const RECEIPT_OK = new Set<unknown>(["success", "0x1", 1, true]);
const RECEIPT_REVERTED = new Set<unknown>(["reverted", "0x0", 0, false]);

const UNFUNDED_SHORT = /insufficient funds|not enough (?:salt|funds|balance)|balance is (?:too low|short)|cannot pay (?:the |for )?(?:deploy )?gas|can't pay (?:the |for )?(?:deploy )?gas/i;

const REQUEST_TOOLS = new Set(["request_status", "tasks_get", "task_status"]);

const lower = (s: unknown): string | null => (typeof s === "string" ? s.toLowerCase() : null);

/** The request/task id a result's top-level object names. */
function idOf(top: Obj): string | null {
  for (const k of ["id", "requestId", "taskId"]) if (typeof top[k] === "string") return top[k] as string;
  if (isObj(top.task) && typeof top.task.taskId === "string") return top.task.taskId;
  return null;
}

/** The receipts at their fixed places: the object itself, `receipt`, `confirmation.receipt`, `task.receipt`. */
function receiptsOf(top: Obj): Obj[] {
  const at = [top, top.receipt, isObj(top.confirmation) ? top.confirmation.receipt : null, isObj(top.task) ? top.task.receipt : null];
  return at.filter((x): x is Obj => isObj(x) && "contractAddress" in x);
}

/**
 * Read where the turn's deploys stand from its tool results, in order, on top of what the
 * conversation already established (`ledger`, updated in place). `memberTexts` are the member's
 * messages in this conversation; `memberWallet` is the wallet the app holds for the member;
 * `contextTexts` is what core told Hermes (its system prompt).
 */
export function collectDeployEvidence(
  outputs: TurnToolOutput[],
  opts: { ledger?: DeployLedger; memberTexts?: string[]; memberWallet?: string | null; contextTexts?: string[] } = {},
): DeployEvidence {
  const ledger = opts.ledger ?? newDeployLedger();
  const wallet = (): string | null => ledger.wallet ?? lower(opts.memberWallet ?? null);
  let state: DeployState = { kind: "none" };
  let unfunded: DeployEvidence["unfunded"] = null;

  for (const o of outputs) {
    const name = baseName(o.tool);
    const content = String(o.content ?? "");
    for (const t of addressLikeTokens(content)) {
      if (ledger.toolStrings.size >= MAX_TOOL_STRINGS) break;
      ledger.toolStrings.add(t.toLowerCase());
    }
    // #681: only core's own tools speak for a deploy, a balance or a receipt.
    if (originOf(o) === "other") continue;
    const top = topObject(content);
    const failed = o.status === "error" || o.status === "denied";

    // Funding: a zero balance read, or a refusal that says the gas cannot be paid. The node's
    // wallet_info also names the member's wallet (an explain_tx receipt from it is theirs).
    if (name === "wallet_info" || name === "get_balance" || name === "wallet_balance") {
      const wei = top ? (top.balanceWei ?? top.balance_wei) : undefined;
      if (wei === "0" || wei === 0) unfunded = "zero";
      if (name === "wallet_info" && !failed && top) {
        const a = top.address;
        if (typeof a === "string" && isWellFormedAddress(a)) ledger.wallet = a.toLowerCase();
      }
    }
    if (UNFUNDED_SHORT.test(content)) unfunded = unfunded ?? "short";

    if (name === "contract_deploy") {
      if (/Deploy refused/i.test(content)) state = { kind: "refused", reason: content };
      else if (/expired/i.test(content) && !/^Proposed/.test(content)) state = { kind: "expired" };
      else if (/^Proposed the contract deploy/.test(content)) state = { kind: "pending" };
      else if (failed || /^couldn't prepare the deploy/i.test(content)) state = { kind: "failed", reason: content };
    } else if (name === "deploy_propose") {
      if (failed || /refused/i.test(content)) {
        state = { kind: /refused/i.test(content) ? "refused" : "failed", reason: content };
      } else {
        // The id this conversation's deploy can be followed by (request_status / tasks_get).
        if (top) {
          for (const k of ["requestId", "taskId", "id"]) if (typeof top[k] === "string") ledger.requestIds.add(top[k] as string);
          if (isObj(top.task) && typeof top.task.taskId === "string") ledger.requestIds.add(top.task.taskId);
        }
        state = { kind: "pending" };
      }
    } else if (REQUEST_TOOLS.has(name)) {
      // Only a request this conversation's deploy_propose opened speaks for its deploy.
      const id = top ? idOf(top) : null;
      if (!top || id === null || !ledger.requestIds.has(id)) continue;
      const confirmation = isObj(top.confirmation) ? top.confirmation : undefined;
      const sentHash = lower(confirmation?.txHash) ?? lower(TX_HASH.exec(content)?.[0] ?? null);
      if (sentHash) ledger.txHashes.add(sentHash);
      let confirmed = false;
      for (const r of receiptsOf(top)) {
        const addr = r.contractAddress;
        const txHash = lower(r.transactionHash) ?? lower(r.txHash);
        if (txHash) ledger.txHashes.add(txHash);
        if (RECEIPT_OK.has(r.status) && typeof addr === "string" && isWellFormedAddress(addr)) {
          ledger.receipts.set(addr.toLowerCase(), txHash);
          state = { kind: "confirmed", address: addr, txHash };
          confirmed = true;
        } else if (RECEIPT_REVERTED.has(r.status)) {
          state = { kind: "reverted", txHash };
          confirmed = true;
        }
      }
      if (confirmed) continue;
      const st = typeof top.state === "string" ? top.state : null;
      if (st === "pending") state = { kind: "pending" };
      else if (st === "rejected" || st === "declined") state = { kind: "declined" };
      else if (st === "expired") state = { kind: "expired" };
      else if (st === "failed") state = { kind: "failed", reason: content };
      else if (confirmation?.state === "waiting" || confirmation?.state === "unknown" || /waiting for the chain to include it/i.test(content)) {
        state = { kind: "sent", txHash: sentHash };
      } else if (confirmation?.state === "reverted" || /reverted/i.test(content)) state = { kind: "reverted", txHash: sentHash };
    } else if (name === "explain_tx" && originOf(o) === "core" && /^Status: success\b/m.test(content)) {
      // core's explain_tx text: a receipt for the member's own deploy only when the transaction is
      // one this conversation's deploy was sent as, or it was sent from the member's wallet. Only
      // the first makes it this turn's confirmed deploy (#681: an old deploy from the wallet is a
      // receipt, not news).
      const created = /^Created contract (0x[0-9a-fA-F]{40})\b/m.exec(content);
      if (!created) continue;
      const txHash = lower(/^explain_tx (0x[0-9a-fA-F]{64})\b/m.exec(content)?.[1] ?? null);
      const from = lower(/^From (0x[0-9a-fA-F]{40})\b/m.exec(content)?.[1] ?? null);
      const bound = txHash !== null && ledger.txHashes.has(txHash);
      if (!bound && !(from !== null && from === wallet())) continue;
      ledger.receipts.set(created[1].toLowerCase(), txHash);
      if (bound) state = { kind: "confirmed", address: created[1], txHash };
    }
  }
  const memberStrings = new Set<string>();
  for (const t of opts.memberTexts ?? []) for (const a of addressLikeTokens(plainText(t))) memberStrings.add(a.toLowerCase());
  const contextStrings = new Set<string>();
  const w = wallet();
  if (w) contextStrings.add(w);
  for (const t of opts.contextTexts ?? []) for (const a of addressLikeTokens(plainText(t))) contextStrings.add(a.toLowerCase());
  const latest = (opts.memberTexts ?? []).at(-1) ?? "";
  const deployRequested = DEPLOY_ASK.test(fold(plainText(latest)));
  return { state, receipts: ledger.receipts, toolStrings: ledger.toolStrings, memberStrings, contextStrings, unfunded, deployRequested };
}

// ---- claims ----

/** Spans that talk about a deploy without claiming one happened (future, conditional, negated). */
const NOT_A_CLAIM =
  /\b(?:not|never)\s+(?:yet\s+)?(?:been\s+)?deployed\b|\b\w+n't\s+(?:yet\s+)?(?:been\s+)?deployed\b|\bno (?:contract|deploy(?:ment)?)\s+(?:was|is|has been|happened|ran)(?:\s+deployed)?\b|\b(?:will|would|can|could|should|may|might|to)\s+be\s+deployed\b|\b(?:once|until|after|before|when|if|unless)\s+(?:it(?:'s| is)?|they(?:'re| are)?|the contract (?:is )?|your contract (?:is )?)?\s*deployed\b|\bnot (?:yet )?(?:live|on[- ]?chain)\b|\bready to (?:be )?deploy(?:ed)?\b|\bdeployed nothing\b|\bnothing (?:was |has been |got )?deployed\b/gi;

/** A sentence (with the non-claim spans taken out) that says a deploy happened. */
const DONE_CLAIM =
  /\b(?:has|have|had)\s+(?:now\s+)?(?:been\s+)?(?:successfully\s+)?deployed\b|\b(?:was|were|is|are|got)\s+(?:now\s+|successfully\s+)*deployed\b|\b(?:successfully|i|i've|we|we've|you've)\s+deployed\b|\bdeployed\s+(?:successfully|it|them|your|the|both|two|a|an)\b|\bdeploy(?:ment)?\s+(?:is\s+|was\s+)?(?:succeeded|complete|completed|successful|done|finished|went through|worked)\b|\b(?:is|are)\s+now\s+live\b|\bwent\s+live\b|\bnow\s+(?:live|on[- ]?chain)\b|\b(?:contract|token|it|they)\b[^.!?\n]{0,40}\b(?:is|are)\s+(?:now\s+)?(?:live|on[- ]?chain)\s+(?:at|on)\b|\b(?:live|on[- ]?chain)\s+(?:at|on)\s+0x|\bsuccessfully\s+(?:launched|created)\b|\bcreated\s+(?:your|the|a|an)\s+(?:new\s+)?(?:\w+\s+)?contract\b|\bcontract\s+(?:was|is|has been)\s+created\b|\b(?:deploy(?:ment)?|contract)\s+(?:is\s+|was\s+|has been\s+)?confirmed\b|\b(?:it|it's|it is|that's|that is|they're|they are|everything is|we're|we are)\s+(?:now\s+)?live\b|\b(?:it|that|everything)\s+went\s+through\b|^\W*(?:now\s+)?live\b|^\W*(?:all\s+|both\s+)?deployed\b/i;

/** Words that tie an address in a sentence to a deployment. */
const DEPLOY_WORD =
  /\bdeployed\b|\bdeployment address(?:es)?\b|\b(?:your|my|our)\s+(?:new\s+)?(?:\w+\s+)?contracts?\b|\blive\s+(?:at|on)\b|\bcontract(?:'s)?\s+address(?:es)?\b|\btoken(?:'s)?\s+(?:contract\s+)?address\b|\bcreated contract\b|\bnew contract\b|\bon[- ]?chain\s+at\b|\b(?:located|lives|available|found)\s+at\b|\bfind\s+(?:it|them)\s+at\b|\bcontract\s+(?:is\s+)?at\b/i;

/** Words that make a sentence about an address (where a malformed one would mislead). */
const ADDRESS_CONTEXT =
  /\baddress(?:es)?\b|\bcontracts?\b|\bwallet\b|\baccount\b|\brecipient\b|\bsend(?:s|ing)?\b[^.!?\n]*\bto\b|\btransfer(?:s|red|ring)?\b[^.!?\n]*\bto\b|\bdeploy\w*\b|\blive\s+(?:at|on)\b|\btoken\b/i;

/** First- or second-person words: the claim is about the member's or Hermes's own deploy. */
const OWN = /\b(?:i|i've|i'm|we|we've|our|you|you've|your|my|me|us)\b/i;

/** A claim with no subject ("Deployed!", "Deployment complete."): Hermes reporting its own deploy. */
const SUBJECTLESS = /^\W*(?:(?:all|both|successfully)\s+)?(?:deployed|deployment|deploy\s+(?:done|complete|succeeded)|done|success)\b/i;

/** A claim about "the deployment", "it", "the contract": in a turn where the member asked for a
 *  deploy and none was attempted, that is Hermes reporting its own (nonexistent) deploy. */
const DEFINITE = /^\W*(?:the|it|it's|its|this|that|they|they're|everything|all)\b/i;

/** A line that labels a value ("Address: 0x…", a list item, a table row). */
const LABEL = /^\s*(?:[-*+|>]|\d+[.)]\s)|^[^.!?:\n]{0,60}:\s*\S/;
/** A list item or table row (its meaning comes from the line that introduces the list). */
const LIST_LINE = /^\s*(?:[-*+|]|\d+[.)]\s)/;

/** The check's own sentences about an address it could not confirm (read again, they claim nothing). */
const OWN_STATUS = /^\s*I can\x27t confirm a deploy at 0x[0-9a-fA-F]{40}: (?:eth_getCode on chain \d+ returned no code there|its code on chain \d+ could not be checked)/;

/**
 * Split text into sentences, keeping every character. A sentence ends at a line break, or at
 * `.`, `!` or `?` that is not followed by a letter or digit ("1.2" and "v0.5" stay whole), and
 * takes the whitespace after its end with it. (#681: the earlier regex split skipped the text
 * before a "1.2"-style dot, so a corrected answer lost it.) The sidecar's port
 * (citrate-agent-runtime agent-sidecar/src/deploy_claims.rs) splits the same way.
 */
export function sentences(text: string): string[] {
  const out: string[] = [];
  const alnum = (c: string | undefined) => c !== undefined && /[0-9A-Za-z]/.test(c);
  let start = 0;
  let i = 0;
  while (i < text.length) {
    const c = text[i];
    const end = c === "\n" || ((c === "." || c === "!" || c === "?") && !alnum(text[i + 1]));
    i++;
    if (end) {
      while (i < text.length && /[ \t\n\f\r]/.test(text[i])) i++;
      out.push(text.slice(start, i));
      start = i;
    }
  }
  if (start < text.length) out.push(text.slice(start));
  return out;
}

function claimText(sentence: string): string {
  return fold(sentence).replace(NOT_A_CLAIM, " ");
}

function gasLine(ev: DeployEvidence): string {
  return ev.unfunded === "zero"
    ? " The wallet holds 0 SALT, so it cannot pay deploy gas yet."
    : ev.unfunded === "short"
      ? " The wallet does not have enough SALT to pay the deploy gas."
      : "";
}

/** The plain statement of where the deploy stands, said in place of a removed claim. */
export function deployStatusLine(ev: DeployEvidence): string {
  const s = ev.state;
  const gas = gasLine(ev);
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
      return `No deploy ran in this conversation turn, so no contract was deployed by it. There is no contract address to report.${gas}`;
  }
}

type CodeVerdict = { kind: "code" } | { kind: "empty" } | { kind: "unknown"; why: string };

function codeLine(addr: string, v: CodeVerdict): string {
  return v.kind === "unknown"
    ? `I can't confirm a deploy at ${addr}: its code on chain 40204 could not be checked (${v.why.slice(0, 200)}).`
    : `I can't confirm a deploy at ${addr}: eth_getCode on chain 40204 returned no code there.`;
}

/**
 * Check one final answer against the evidence. `readCode` reads `eth_getCode` at an address on
 * chain 40204; a failed read is "could not be checked", never code.
 */
export async function checkDeployClaims(
  answer: string,
  ev: DeployEvidence,
  readCode: (address: string) => Promise<CodeRead>,
): Promise<DeployClaimCheck> {
  const codeCache = new Map<string, Promise<CodeVerdict>>();
  const code = (addr: string): Promise<CodeVerdict> => {
    const k = addr.toLowerCase();
    let p = codeCache.get(k);
    if (!p) {
      p =
        codeCache.size >= MAX_CODE_READS
          ? Promise.resolve<CodeVerdict>({ kind: "unknown", why: `this answer names more than ${MAX_CODE_READS} addresses` })
          : readCode(addr).then(
              (c): CodeVerdict => (c.isContract && c.codeSize > 0 ? { kind: "code" } : { kind: "empty" }),
              (e): CodeVerdict => ({ kind: "unknown", why: e instanceof Error ? e.message : String(e) }),
            );
      codeCache.set(k, p);
    }
    return p;
  };
  const attempted = ev.state.kind !== "none";
  const confirmed = ev.state.kind === "confirmed";
  const written = (k: string) => ev.toolStrings.has(k) || ev.memberStrings.has(k) || ev.contextStrings.has(k);
  const findings: string[] = [];
  const status: string[] = [];
  const say = (line: string) => {
    if (!status.includes(line)) status.push(line);
  };
  const out: string[] = [];
  const text = plainText(answer);
  let prev = "";
  let lead = "";

  for (const sentence of sentences(text)) {
    const claim = claimText(sentence);
    const listLine = LIST_LINE.test(sentence);
    // The sentence before, and the line that introduces a list or table, give a bare
    // "Address: 0x…" or a table cell its meaning.
    const around = prev + " " + (listLine ? lead : "");
    const tokens = OWN_STATUS.test(sentence) ? [] : addressLikeTokens(sentence).filter((t) => addressShape(t) !== "other");
    const deployClaimed = DONE_CLAIM.test(claim);
    const ctxClaimed = deployClaimed || (LABEL.test(sentence) && DONE_CLAIM.test(claimText(around)));
    const inClaim = tokens.length > 0 && (ctxClaimed || DEPLOY_WORD.test(claim) || (LABEL.test(sentence) && DEPLOY_WORD.test(claimText(around))));
    const own =
      OWN.test(claim) ||
      SUBJECTLESS.test(claim) ||
      (ev.deployRequested && !attempted && DEFINITE.test(claim)) ||
      (LABEL.test(sentence) && (OWN.test(claimText(around)) || SUBJECTLESS.test(claimText(prev)) || SUBJECTLESS.test(claimText(lead))));
    let drop = false;
    const replace = new Map<string, string>();

    for (const t of tokens) {
      const k = t.toLowerCase();
      if (addressShape(t) === "malformed") {
        if (written(k)) continue;
        if (inClaim) {
          findings.push(`removed a deploy claim with ${t}, which is not a valid address (${t.length - 2} characters after 0x, or non-hex characters)`);
          say(deployStatusLine(ev));
          drop = true;
        } else if (ADDRESS_CONTEXT.test(fold(sentence))) {
          findings.push(`removed ${t}: not a valid address (0x + 40 hex characters)`);
          replace.set(t, REMOVED_ADDRESS);
        }
        continue;
      }
      // A valid address.
      if (ev.receipts.has(k)) {
        if (!inClaim) continue;
        const v = await code(t);
        if (v.kind === "code") continue;
        findings.push(`removed a deploy claim at ${t}: a trusted receipt names it, but ${v.kind === "empty" ? "eth_getCode returned no code there" : "its code could not be checked"}`);
        say(codeLine(t, v));
        drop = true;
        continue;
      }
      if (attempted && !written(k)) {
        findings.push(`removed ${t}: this turn attempted a deploy and no tool returned that address`);
        say(deployStatusLine(ev));
        if (inClaim) drop = true;
        else replace.set(t, REMOVED_UNGROUNDED);
        continue;
      }
      if (inClaim && ctxClaimed && attempted && !confirmed) {
        // #681: this turn's deploy is not confirmed, so nothing it names is deployed yet.
        findings.push(`removed a deploy claim at ${t}: this turn's deploy is not confirmed`);
        say(deployStatusLine(ev));
        drop = true;
        continue;
      }
      if (inClaim && own) {
        // The member's or Hermes's own deploy: a trusted receipt (above), or an address the member
        // wrote themselves with code at it, in a turn that attempted no deploy.
        if (!attempted && ev.memberStrings.has(k)) {
          const v = await code(t);
          if (v.kind === "code") continue;
          findings.push(`removed a claim about the member's contract at ${t}: ${v.kind === "empty" ? "no code there" : "its code could not be checked"}`);
          say(codeLine(t, v));
          drop = true;
          continue;
        }
        findings.push(`removed a deploy claim at ${t}: no trusted deploy receipt in this conversation names it`);
        say(deployStatusLine(ev));
        drop = true;
        continue;
      }
      if (!inClaim && written(k)) continue;
      // Someone else's contract, or (#681) an address nothing in this conversation produced,
      // however it is worded: grounded only when there is code at it.
      const v = await code(t);
      if (v.kind === "code") continue;
      if (inClaim) {
        findings.push(`removed a deploy claim at ${t}: ${v.kind === "empty" ? "eth_getCode returned no code there" : "its code could not be checked"}`);
        say(codeLine(t, v));
        drop = true;
      } else {
        findings.push(`removed ${t}: no tool returned it and ${v.kind === "empty" ? "eth_getCode returned no code there" : "its code could not be checked"}`);
        replace.set(t, v.kind === "empty" ? REMOVED_NO_CODE : REMOVED_UNCHECKED);
      }
    }

    if (!drop && tokens.length === 0 && deployClaimed && !confirmed && (attempted || own) && !(own && !attempted && ev.receipts.size > 0)) {
      findings.push(`removed "${sentence.trim().slice(0, 120)}": no trusted deploy receipt in this turn`);
      say(deployStatusLine(ev));
      drop = true;
    }

    if (sentence.trim() !== "") {
      prev = sentence;
      if (!listLine) lead = sentence;
    }
    if (drop) {
      // Keep the sentence's line break so the rest of the answer keeps its shape.
      out.push(/\n\s*$/.test(sentence) ? "\n" : "");
      continue;
    }
    out.push(replace.size ? sentence.replace(ADDRESS_LIKE, (t) => replace.get(t) ?? t) : sentence);
  }

  if (findings.length === 0) return { text: answer, changed: false, findings };
  let result = out.join("").replace(/\n{3,}/g, "\n\n").trim();
  if (status.length) {
    const line = status.join(" ") + (status.some((s) => s.includes("SALT")) ? "" : gasLine(ev));
    result = result ? `${result}\n\n${line}` : line;
  }
  return { text: result, changed: true, findings };
}
