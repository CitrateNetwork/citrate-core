// =====================================================================
// citrate-core: "What Hermes can see" in this chat (FL design board 01, "Context is inspectable")
//
// Derived only from what this chat's Hermes was really given: the tools core sent to its session
// (store.chatToolNames), which loop answers (the sidecar loop also runs its own file, command and
// MCP tools, each command behind the member's approval), and the model. A row the session has no
// tool for says "no". Keys are never visible: signing happens only in the ceremony.
//
// Turning a row off for one chat, @ context and attaching datasets are not built yet (core #690:
// the session would need to be reopened with fewer tools); this panel only reports.
// =====================================================================

export interface ContextRow {
  id: string;
  title: string;
  detail: string;
  /** Whether Hermes can see it in this chat. */
  can: boolean;
}

export interface ContextInput {
  toolNames: readonly string[];
  /** ChatProvider.kind: sidecar / sidecar-cloud (the agent loop), agent (gateway loop), others none. */
  providerKind: string;
  /** The chat is on the no-tools fallback (Hermes not running, loop off, status unknown). */
  hermesDown: boolean;
  modelLabel: string;
}

const GROUPS: { id: string; title: string; tools: string[]; detail: string }[] = [
  { id: "node", title: "This node", tools: ["node_status", "staking_status"], detail: "status, height, peers and your staking position, read only" },
  {
    id: "chain",
    title: "Network reads",
    tools: ["contract_view", "get_code", "identify_contract", "address_book_lookup", "explain_tx", "decode_calldata", "decode_log", "models_list", "skills_list", "get_verified_source"],
    detail: "contracts, transactions and registries on this network",
  },
  { id: "memory", title: "Your notes and journal", tools: ["memory_search", "memory_recall", "journal_read"], detail: "read; saving a note asks you first" },
  { id: "groups", title: "Your groups", tools: ["groups_list", "group_roster"], detail: "names and rosters, not their messages" },
  { id: "rounds", title: "Round coordinator", tools: ["fl_round_plan"], detail: "round plans; joining asks you (HIC-1)" },
  { id: "google", title: "Google sheets and calendar", tools: ["gsheets_read", "calendar_list"], detail: "only after you connect Google in Settings" },
];

export function contextRows(i: ContextInput): ContextRow[] {
  const has = new Set(i.toolNames);
  const loop = !i.hermesDown && (i.providerKind === "sidecar" || i.providerKind === "sidecar-cloud");
  const rows: ContextRow[] = GROUPS.map((g) => {
    const can = g.tools.some((t) => has.has(t));
    return { id: g.id, title: g.title, detail: can ? g.detail : "no tool for it in this chat", can };
  });
  rows.push({
    id: "folders",
    title: "Folders and commands",
    detail: loop ? "only folders you shared in Hermes › Controls; every command asks you" : "not in this chat (the Hermes agent loop is not answering)",
    can: loop,
  });
  rows.push({ id: "datasets", title: "Datasets", detail: "none; attaching a dataset to a chat is not available yet", can: false });
  rows.push({ id: "keys", title: "Your wallet keys", detail: "never; signing happens only in the signature ceremony", can: false });
  return rows;
}

/** The one line above the rows: which model answers and whether it has tools at all. */
export function contextSummary(i: ContextInput): string {
  if (i.hermesDown) return `Replies come from ${i.modelLabel} with no tools, so Hermes cannot read anything here.`;
  if (i.toolNames.length === 0) return `${i.modelLabel} answers without tools in this chat, so Hermes cannot read anything here.`;
  return `${i.modelLabel} answers, with ${i.toolNames.length} core ${i.toolNames.length === 1 ? "tool" : "tools"}.`;
}
