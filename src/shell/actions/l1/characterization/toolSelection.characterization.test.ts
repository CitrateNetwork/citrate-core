// L1-WP01 CH-4 (characterization, citrate-core a826e441). LQ registry: surface "harness and
// provider selection".
//
// Pins the in-app tool schema selection (`selectToolSchemas` and `TOOL_SCHEMA_CEILING`,
// src/agent/harness.ts) on a fixed request corpus: at most 8 schemas, tools already used this
// turn first (most recent first, unknown names skipped, no repeats), then the catalog ranked by
// how many request words appear in each tool's name and description, ties in catalog order. The
// expected names are today's output, including its oddities (for example widget_create ranking
// first for a node status question). WP17 (tool search, compact prompt) changes this and names CH-4.
import { describe, it, expect } from "vitest";
import { AGENT_TOOLS, TOOL_SCHEMA_CEILING, selectToolSchemas } from "../../../../agent/harness";

const CORPUS: { request: string; used: string[]; names: string[] }[] = [
  {
    request: "what is my node status and peers",
    used: [],
    names: ["widget_create", "node_status", "staking_status", "group_create", "cluster_read", "contract_deploy", "address_book_lookup", "get_code"],
  },
  {
    request: "deploy my contract to the chain",
    used: [],
    names: ["contract_deploy", "address_book_lookup", "get_code", "identify_contract", "staking_status", "skills_list", "skill_run", "models_list"],
  },
  {
    request: "deploy a contract and check the deploy gate",
    used: [],
    names: ["contract_deploy", "fl_claim_propose", "get_verified_source", "contract_view", "address_book_lookup", "get_code", "identify_contract", "decode_calldata"],
  },
  {
    request: "search the docs for the ghostdag parameters",
    used: [],
    names: ["memory_search", "memory_recall", "app_navigate", "memory_assert", "journal_append", "journal_read", "node_status", "staking_status"],
  },
  {
    request: "list my calendar events for this week",
    used: [],
    names: ["calendar_list", "groups_list", "group_roster", "group_invites_list", "group_invite_revoke", "skills_list", "skill_run", "models_list"],
  },
  {
    request: "join the federated learning round",
    used: [],
    names: ["fl_consent_list", "fl_round_draft_read", "fl_history_read", "fl_round_plan", "fl_offers_list", "fl_payouts_list", "fl_round_status", "fl_lease_explain"],
  },
  {
    request: "",
    used: [],
    names: ["memory_search", "memory_recall", "app_navigate", "memory_assert", "journal_append", "journal_read", "node_status", "staking_status"],
  },
  {
    request: "what is my node status and peers",
    used: ["calendar_list"],
    names: ["calendar_list", "widget_create", "node_status", "staking_status", "group_create", "cluster_read", "contract_deploy", "address_book_lookup"],
  },
  {
    request: "read my journal",
    used: ["journal_read"],
    names: ["journal_read", "schedule_list", "journal_append", "node_status", "staking_status", "groups_list", "group_roster", "group_join_propose"],
  },
  {
    request: "hello",
    used: ["nope_not_a_tool", "node_status", "node_status"],
    names: ["node_status", "memory_search", "memory_recall", "app_navigate", "memory_assert", "journal_append", "journal_read", "staking_status"],
  },
];

const names = (tools: readonly { function: { name: string } }[]) => tools.map((t) => t.function.name);

describe("CH-4: tool schema selection on a fixed corpus", () => {
  it("the ceiling and the catalog size", () => {
    expect(TOOL_SCHEMA_CEILING).toBe(8);
    expect(AGENT_TOOLS.length).toBe(74);
  });

  for (const c of CORPUS) {
    it(`${JSON.stringify(c.request)} used=${JSON.stringify(c.used)}`, () => {
      expect(names(selectToolSchemas(AGENT_TOOLS, c.request, c.used))).toEqual(c.names);
    });
  }

  it("never more than the ceiling, never a repeat, used tools first", () => {
    for (const c of CORPUS) {
      for (const max of [0, 1, 3, TOOL_SCHEMA_CEILING, 20]) {
        const got = names(selectToolSchemas(AGENT_TOOLS, c.request, c.used, max));
        expect(got.length).toBe(Math.min(max, AGENT_TOOLS.length));
        expect(new Set(got).size).toBe(got.length);
        const known = [...new Set(c.used.filter((n) => AGENT_TOOLS.some((t) => t.function.name === n)))];
        expect(got.slice(0, Math.min(max, known.length))).toEqual(known.slice(0, max));
      }
    }
  });
});
