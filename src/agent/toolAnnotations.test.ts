// HUP-S2.4 / A8 — every chat-agent tool carries the runtime's taint annotations (effect + trust).
// The sidecar (citrate-agent-runtime, HUP-S2.7) treats an unannotated tool as effectful and its
// output as untrusted; core keeps ONE reviewed annotation per tool so the downgrade is precise.
// Enumeration tripwire: a new tool without an annotation, or an effectful tool marked `none`, fails.
import { describe, it, expect, vi, afterEach } from "vitest";
import { AGENT_TOOLS, AUTHORITY_LOWERING_AGENT_TOOLS, READ_ONLY_AGENT_TOOLS, type ToolCall } from "./harness";
import { AGENT_TOOL_ANNOTATIONS, annotatedAgentTools, annotationFor } from "./toolAnnotations";
import { store } from "../shell/store";
import { bridge } from "../bridge";
import { livePlan } from "../fl/fixtures/plan";
import { ADAPTER_SHA, flOverview, gateRecord } from "../fl/fixtures/gate";
import { DATASET_ID, cleanPreview, datasetList, publishPreview } from "../fl/fixtures/datasets";
import { FILLED_DRAFT, ROUND_BINDING, VRAM_24GB_REPORT, oneOfferMarket, payoutsOpenNetwork } from "../fl/fixtures/market";
import { saveRoundDraft } from "../fl/roundDraft";
import { flCollect } from "../fl/collect";
import { collectPlan } from "../fl/fixtures/collect";

const names = AGENT_TOOLS.map((t) => t.function.name as string);
const EFFECTS = ["none", "write", "spend", "sign"];
const TRUSTS = ["trusted", "untrusted"];

afterEach(() => vi.restoreAllMocks());

describe("Feature: every agent tool is annotated (A8)", () => {
  it("Given AGENT_TOOLS, then every entry has a valid effect and trust, and no annotation names a missing tool", () => {
    for (const n of names) {
      const a = annotationFor(n);
      expect(a, `tool ${n} has no annotation`).not.toBeNull();
      expect(EFFECTS).toContain(a!.effect);
      expect(TRUSTS).toContain(a!.trust);
    }
    for (const k of Object.keys(AGENT_TOOL_ANNOTATIONS)) expect(names).toContain(k);
    expect(annotationFor("no_such_tool")).toBeNull();
  });

  it("Given the reviewed read-only list, then a tool is `effect: none` exactly when it is on it", () => {
    for (const n of names) {
      expect(annotationFor(n)!.effect === "none", `tool ${n}`).toBe(READ_ONLY_AGENT_TOOLS.has(n));
    }
  });

  it("Given a tool that stops at a member approval when invoked, then it is never annotated `none` (and a `none` tool never asks)", async () => {
    const call = (name: string): ToolCall => ({
      id: "c1",
      name,
      arguments: JSON.stringify({
        group: "g",
        name: "n",
        bytecodeHex: "0x00",
        fact: "f",
        entry: "e",
        instructions: "i",
        query: "q",
        html: "<p>x</p>",
        // HUP-S10.2: the everyday tools' arguments (a valid sheet, rows and a future start).
        spreadsheetId: "1AbCdEfGhIjKlMnOpQrStUvWxYz012345",
        range: "Sheet1!A:B",
        rows: [["a", 1]],
        title: "Weekly review",
        start: "2099-01-05T09:00",
        // 0.5.4 WP-H4: the envelope switch and caps.
        enabled: true,
        joinsPerWeek: 3,
        maxHoursPerRound: 8,
        // 0.5.4 WP-H2: an invite link, a roster member, a role and the mesh switch.
        link: "citrate://invite?g=g&t=t&k=k&e=9",
        member: "0x" + "1".repeat(40),
        role: "admin",
        sha256: ADAPTER_SHA,
        // 0.5.4 WP-H6: a dataset create from a granted folder, and a clean of a listed dataset.
        dataClass: "orchard-notes",
        folder: "/home/m/notes",
        dataset: DATASET_ID,
        ops: [{ op: "dedupe" }],
        offerId: 1,
        license: "MIT",
      }),
    });
    for (const n of names) {
      vi.restoreAllMocks();
      const sig = vi.spyOn(store, "requestSig").mockResolvedValue("declined");
      const review = vi.spyOn(store, "openWalletReview").mockImplementation(() => {});
      vi.spyOn(store, "go").mockImplementation(() => {});
      vi.spyOn(bridge.agentSkills, "write").mockRejectedValue(new Error("SKILL_EXISTS: exists"));
      vi.spyOn(bridge.agentSkills, "read").mockResolvedValue("old");
      vi.spyOn(bridge.contracts, "deploy").mockResolvedValue({ id: "cer1", origin: "o", kind: "transaction", chainId: 40204, decoded: { action: "a", cost: "c", destination: "d" }, requiresRawAck: false } as never);
      // HUP-S9.4: fl_round_start asks only about a plan core built, so give it one that can start.
      vi.spyOn(bridge.flRounds, "lookupPlan").mockResolvedValue(livePlan());
      // 0.5.4 WP-H3: no terms saved yet.
      vi.spyOn(store, "flWorkTerms").mockReturnValue({ load: async () => null });
      // 0.5.4 WP-H4: an envelope status core could return on r1007 (WorkMarket not pinned).
      vi.spyOn(store, "flEnvelopeApi").mockReturnValue({
        status: async () => ({ scope: "fl_envelope", envelope: null, joinsLeft: null, workMarketPinned: false, authorizes: false, note: "n", rules: [] }),
        approve: async () => ({}) as never,
        off: async () => null,
      });
      // 0.5.4 WP-H2: a readable invite, a roster with the member, a signed-off mesh that is off.
      vi.spyOn(bridge.invites, "preview").mockResolvedValue({ group: "g", groupName: "G", inviter: "A", expiresAt: 9, expired: false, requested: false });
      vi.spyOn(bridge.invites, "revoke").mockResolvedValue({ notified: false });
      vi.spyOn(bridge.groups, "roster").mockResolvedValue([{ address: "0x" + "1".repeat(40), role: "member" }]);
      vi.spyOn(bridge.cluster, "meshStatus").mockResolvedValue({ on: false, source: "off", signedOff: true, note: "" });
      // 0.5.4 WP-H9: an adapter the gate accepted, and an unload core completes.
      vi.spyOn(bridge.flRounds, "overview").mockResolvedValue(flOverview({ gates: [gateRecord()] }));
      vi.spyOn(bridge.flRounds, "unloadAdapter").mockResolvedValue(true);
      // 0.5.4 WP-H6: the dataset writes ask about a dataset core lists and a clean core previewed.
      vi.spyOn(bridge.flDatasets, "list").mockResolvedValue(datasetList());
      vi.spyOn(bridge.flDatasets, "cleanPreview").mockResolvedValue(cleanPreview());
      // 0.5.4 WP-H5: fl_join_propose reaches the wallet review only on a 0.6.0 network with payouts open.
      vi.spyOn(store, "flNetworkState").mockImplementation(payoutsOpenNetwork);
      vi.spyOn(store, "flMarketOffers").mockResolvedValue(oneOfferMarket());
      vi.spyOn(bridge.tier, "recommend").mockResolvedValue(VRAM_24GB_REPORT);
      // 0.5.4 WP-H5: fl_round_open_propose reaches the wallet review with a draft and a binding (test-only).
      vi.spyOn(store, "flRoundBinding").mockResolvedValue(ROUND_BINDING);
      store.setState({ commsAddr: "0xme" });
      vi.spyOn(bridge.groups, "list").mockResolvedValue([{ id: FILLED_DRAFT.groupId, name: "g", owner: "o", kind: "channel", members: [{ address: "0xme", role: "owner" }] }] as never);
      saveRoundDraft({ ...FILLED_DRAFT });
      vi.spyOn(bridge.contracts, "proposeWrite").mockResolvedValue({ id: "cer1" } as never);
      // 0.5.4 WP-H10: fl_withdraw_propose reaches the wallet review once payouts are open and the pull balance is not 0.
      vi.spyOn(bridge.contracts, "viewCall").mockResolvedValue("0x" + (10n ** 18n).toString(16).padStart(64, "0"));
      store.setState({ walletAddr: "0x" + "5b".repeat(20) });
      // WP-H8: fl_collect_plan asks only about a plan core built with something to collect.
      vi.spyOn(flCollect, "plan").mockResolvedValue(collectPlan());
      vi.spyOn(flCollect, "decide").mockResolvedValue(collectPlan({ status: "declined" }));
      vi.spyOn(bridge.flDatasets, "published").mockResolvedValue([]);
      vi.spyOn(bridge.flDatasets, "publishPreview").mockResolvedValue(publishPreview());
      // HUP-S10.2: the everyday tools run in the desktop app against a connected Google account.
      vi.spyOn(store, "everydayInvoke").mockReturnValue((async (cmd: string) =>
        cmd === "google_workspace_status"
          ? [
              { service: "gsheets", configured: true, connected: true, note: null },
              { service: "gcal", configured: true, connected: true, note: null },
            ]
          : cmd === "hermes_schedule_list"
            ? { status: "ok", error: null, entries: [], occurrences: [] }
            : cmd === "gcal_list"
              ? []
              : { range: "Sheet1!A:B", rows: [], truncated: false }) as never);
      await store.handleTool(call(n), "m1", () => {});
      const gated = sig.mock.calls.length + review.mock.calls.length > 0;
      if (gated) expect(annotationFor(n)!.effect, `tool ${n} asks the member, so it has an effect`).not.toBe("none");
      // An authority-lowering tool changes something without asking (reviewed list in harness.ts).
      else if (AUTHORITY_LOWERING_AGENT_TOOLS.has(n)) expect(annotationFor(n)!.effect, `tool ${n} lowers authority`).toBe("write");
      else expect(annotationFor(n)!.effect, `tool ${n} never asks, so it must be read-only`).toBe("none");
    }
  });

  it("Given the sidecar session, then the annotated tool list carries {effect, trust} beside each OpenAI wrapper", () => {
    const tools = annotatedAgentTools();
    expect(tools).toHaveLength(AGENT_TOOLS.length);
    for (const t of tools) {
      expect(t.type).toBe("function");
      expect(t.annotations).toEqual(AGENT_TOOL_ANNOTATIONS[t.function.name]);
    }
    // the gateway / local tool loops keep sending the plain OpenAI shapes
    expect(JSON.stringify(AGENT_TOOLS)).not.toContain("annotations");
  });

  it("Given content written by people other than the member, then the tools that return it are untrusted", () => {
    for (const n of ["skills_list", "models_list", "directory_find", "groups_list", "group_roster", "skill_run"]) {
      expect(annotationFor(n)!.trust, n).toBe("untrusted");
    }
    expect(annotationFor("contract_deploy")!.effect).toBe("sign");
  });
});

describe("HUP-S6.7 — contract_view", () => {
  it("is a read with untrusted output (values the contract chose)", () => {
    expect(annotationFor("contract_view")).toEqual({ effect: "none", trust: "untrusted" });
  });
});

describe("HUP-S4.3 — get_verified_source", () => {
  it("is a read with untrusted output (deployer-written source)", () => {
    expect(annotationFor("get_verified_source")).toEqual({ effect: "none", trust: "untrusted" });
  });
});
