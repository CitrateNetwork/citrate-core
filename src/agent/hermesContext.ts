// =====================================================================
// citrate-core: "What Hermes can see" in this chat (FL design board 01, "Context is inspectable")
//
// Derived only from what this chat's Hermes was really given: the tools core sent to its session
// (store.chatToolNames), which loop answers (the sidecar loop also runs its own file, command and
// MCP tools, each command behind the member's approval), and the model. A row the session has no
// tool for says "no". Keys are never visible: signing happens only in the ceremony.
//
// #690: the member can turn a kind of context off for one chat. Core then refuses that kind's
// tools in this chat (store.handleTool, a structured refusal) and tells Hermes so in front of each
// message. Folders and commands are the sidecar's own tools, so they are changed in Hermes ›
// Controls, not here; wallet keys are never visible. Datasets are visible only when the member
// attaches one with @ (chatAttachments.ts).
// =====================================================================

export interface ContextRow {
  id: string;
  title: string;
  detail: string;
  /** Whether Hermes can see it in this chat. */
  can: boolean;
  /** Whether the member can switch it off (and on) for this chat here. */
  toggle: boolean;
  /** The member turned it off for this chat. */
  off: boolean;
}

export interface ContextInput {
  toolNames: readonly string[];
  /** ChatProvider.kind: sidecar / sidecar-cloud (the agent loop), agent (gateway loop), others none. */
  providerKind: string;
  /** The chat is on the no-tools fallback (Hermes not running, loop off, status unknown). */
  hermesDown: boolean;
  modelLabel: string;
  /** Row ids the member turned off for this chat. */
  off?: readonly string[];
  /** Datasets attached to this chat's messages. */
  datasetsAttached?: number;
}

/** The context kind a core tool belongs to (null = not a switchable kind). */
export function contextKindOf(tool: string): { id: string; title: string } | null {
  const g = GROUPS.find((x) => x.tools.includes(tool));
  return g ? { id: g.id, title: g.title } : null;
}

/** The titles of the switchable kinds in `off`. */
export function offTitles(off: readonly string[]): string[] {
  return GROUPS.filter((g) => off.includes(g.id)).map((g) => g.title);
}

const GROUPS: { id: string; title: string; tools: string[]; detail: string }[] = [
  { id: "node", title: "This node", tools: ["node_status", "staking_status"], detail: "status, height, peers and your staking position, read only" },
  {
    id: "chain",
    title: "Network reads",
    tools: ["contract_view", "get_code", "identify_contract", "address_book_lookup", "explain_tx", "decode_calldata", "decode_log", "models_list", "skills_list", "get_verified_source"],
    detail: "contracts, transactions and registries on this network",
  },
  { id: "memory", title: "Your notes and journal", tools: ["memory_search", "memory_recall", "journal_read", "memory_assert", "journal_append"], detail: "read; saving a note asks you first" },
  { id: "groups", title: "Your groups", tools: ["groups_list", "group_roster", "group_create", "group_invite"], detail: "names and rosters, not their messages" },
  { id: "rounds", title: "Round coordinator", tools: ["fl_round_plan", "fl_round_start", "worker_terms_propose"], detail: "round plans; joining asks you (HIC-1)" },
  { id: "google", title: "Google sheets and calendar", tools: ["gsheets_read", "calendar_list", "gsheets_append"], detail: "only after you connect Google in Settings" },
];

export function contextRows(i: ContextInput): ContextRow[] {
  const has = new Set(i.toolNames);
  const loop = !i.hermesDown && (i.providerKind === "sidecar" || i.providerKind === "sidecar-cloud");
  const off = new Set(i.off ?? []);
  const rows: ContextRow[] = GROUPS.map((g) => {
    const offered = g.tools.some((t) => has.has(t));
    const isOff = offered && off.has(g.id);
    const detail = !offered ? "no tool for it in this chat" : isOff ? "off for this chat; core refuses these tools here" : g.detail;
    return { id: g.id, title: g.title, detail, can: offered && !isOff, toggle: offered && !i.hermesDown, off: isOff };
  });
  rows.push({
    id: "folders",
    title: "Folders and commands",
    detail: loop ? "only folders you shared in Hermes › Controls (change them there); every command asks you" : "not in this chat (the Hermes agent loop is not answering)",
    can: loop,
    toggle: false,
    off: false,
  });
  const ds = i.datasetsAttached ?? 0;
  rows.push({
    id: "datasets",
    title: "Datasets",
    detail: ds > 0 ? `${ds} attached in this chat: name, size and hash only, never their lines` : "none attached; type @ to attach one",
    can: ds > 0,
    toggle: false,
    off: false,
  });
  rows.push({ id: "keys", title: "Your wallet keys", detail: "never; signing happens only in the signature ceremony", can: false, toggle: false, off: false });
  return rows;
}

/** The one line above the rows: which model answers and whether it has tools at all. */
export function contextSummary(i: ContextInput): string {
  if (i.hermesDown) return `Replies come from ${i.modelLabel} with no tools, so Hermes cannot read anything here.`;
  if (i.toolNames.length === 0) return `${i.modelLabel} answers without tools in this chat, so Hermes cannot read anything here.`;
  return `${i.modelLabel} answers, with ${i.toolNames.length} core ${i.toolNames.length === 1 ? "tool" : "tools"}.`;
}
