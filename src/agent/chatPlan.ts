// =====================================================================
// citrate-core — the plan of a plain chat turn (HUP-S7.6 follow-up, US-7.4 AC1)
//
// A workflow run reports its plan once, from the sidecar's `plan` event (its step ids). A plain
// chat turn has no workflow, so its plan is what the model itself asked for, step by step: each
// model step that requests tools becomes one plan row naming those tools, in the order the model
// asked for them. A row is added only when the provider reports the model's tool call, so the plan
// never lists a step the model did not take (Rule 1). Nothing here runs a tool or decides one.
//
// Data source (Rule 7): the provider's own tool-call reports, the sidecar session's `tool_call`
// events (sidecarProvider) and the model's `tool_calls` in the in-app loop (harness), together with
// the step number the same provider reports.
// =====================================================================

/** At most this many plan rows are kept for one chat turn (matches the slice's MAX_PLAN_STEPS). */
export const MAX_CHAT_PLAN_ROWS = 50;
/** At most this many tool names are listed on one row; the rest are counted. */
export const MAX_TOOLS_PER_ROW = 6;

interface Row {
  step: number;
  tools: string[];
  /** Call ids already counted on this row (a replayed event is not counted twice). */
  calls: Set<string>;
}

/** One row's text: "Step 2: web_search, fetch_url" (repeats are counted, "file_write x2"). */
export function chatPlanLabel(step: number, tools: string[]): string {
  const counts = new Map<string, number>();
  for (const t of tools) counts.set(t, (counts.get(t) ?? 0) + 1);
  const names = [...counts.entries()].map(([t, n]) => (n > 1 ? `${t} x${n}` : t));
  const shown = names.slice(0, MAX_TOOLS_PER_ROW);
  const more = names.length > shown.length ? `, and ${names.length - shown.length} more` : "";
  return `Step ${step}: ${shown.join(", ")}${more}`;
}

/** The plan of one chat turn, built from the tool calls the model asked for. */
export class ChatPlan {
  private rows: Row[] = [];

  /**
   * Note one tool call the model asked for in `step`. Returns true when the plan changed (a new
   * call), false for a replayed call id, an empty tool name, a bad step or a full plan.
   */
  note(step: number, callId: string, tool: string): boolean {
    if (!Number.isInteger(step) || step < 1 || tool.length === 0) return false;
    // Call ids are unique within a step only (the sidecar reuses call_0, call_1 on later steps).
    let row = this.rows.find((r) => r.step === step);
    if (row?.calls.has(callId)) return false;
    if (!row) {
      if (this.rows.length >= MAX_CHAT_PLAN_ROWS) return false;
      row = { step, tools: [], calls: new Set() };
      this.rows.push(row);
      this.rows.sort((a, b) => a.step - b.step);
    }
    row.tools.push(tool);
    row.calls.add(callId);
    return true;
  }

  /** The plan rows as the monitor shows them, in step order. */
  steps(): string[] {
    return this.rows.map((r) => chatPlanLabel(r.step, r.tools));
  }
}
