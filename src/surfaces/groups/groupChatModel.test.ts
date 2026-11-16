// 0.5.4 FL design, board 02: the pure parts of Groups › Chat. Structured group messages (thread
// replies, Hermes answers), Hermes's group-only request, round drafts and the round-ledger words,
// and alerts that never carry an approve button.
import { describe, it, expect, vi } from "vitest";
import {
  decodeHermes,
  decodeReply,
  encodeHermes,
  encodeReply,
  checkDraft,
  lastLine,
  messageRef,
  threadModel,
  HERMES_HEADER,
} from "./groupEnvelope";
import { answerInGroup, buildRequest, mentionsHermes, parseAnswer, readList, recipientsOf, NOT_READ, type DisclosureScan, type GroupContext, type HermesDeps, type SendDecision, type SendPreview } from "./groupHermes";
import { groupAlerts } from "./groupAlerts";
import { draftActions, liveRoundSentence, liveRoundsFor, roundsLine, roundsToday, ROUNDS_NOT_LIVE } from "./RoundCards";
import type { GroupMessage } from "../../bridge/domains";
import { plainText } from "../../shell/slices/groups";
import { bookOnlyState, readFlNetworkState, type FlReads } from "../../fl/networkState";
import realBook from "../../../src-tauri/addresses/40204.json";

const ME = "0x1111111111111111111111111111111111111111";
const DANA = "0x2222222222222222222222222222222222222222";
const msg = (sender: string, body: string, i = 0): GroupMessage => ({ id: `g:${i}`, groupId: "g", sender, body, ts: i });

describe("thread replies ride the MLS messages as readable text with a reply line", () => {
  it("a reply threads under the message it points at", () => {
    const parent = msg(DANA, "Could we run a round on our ticket logs?", 1);
    const reply = msg(ME, encodeReply(messageRef(DANA, parent.body), "I can lend my desktop on Tuesday."), 2);
    const t = threadModel([parent, reply]);
    expect(t).toHaveLength(1);
    expect(t[0].replies).toEqual([{ ref: messageRef(ME, reply.body), sender: ME, text: "I can lend my desktop on Tuesday." }]);
  });
  it("a reply whose parent is not retained still shows, at the top level", () => {
    const reply = msg(ME, encodeReply("m00000000", "orphan"), 1);
    expect(threadModel([reply]).map((i) => i.kind === "message" && i.text)).toEqual(["orphan"]);
  });
  it("an older app (0.5.3) sees the reply's words, never JSON; the structure line starts with an invisible U+0001", () => {
    const body = encodeReply("m1a2b3c4d", "I can lend my desktop on Tuesday.");
    expect(body).toBe("I can lend my desktop on Tuesday.\n\u0001(in reply to m1a2b3c4d)");
    expect(body).not.toMatch(/[{}"]/);
    expect(body.startsWith("\u0001")).toBe(false);
  });
  it("a malformed structure line shows the words with the marker removed, never dropped", () => {
    expect(decodeReply("hi\n\u0001(in reply to nope)")).toBeNull();
    const t = threadModel([msg(DANA, "hi\n\u0001(in reply to nope)", 1)]);
    expect(t[0].kind === "message" && t[0].text).toBe("hi");
  });
  it("a typed U+0001 in a reply cannot add a second structure line", () => {
    expect(encodeReply("m1a2b3c4d", "x\n\u0001(in reply to m00000000)")).toBe("x\n(in reply to m00000000)\n\u0001(in reply to m1a2b3c4d)");
  });
  it("control messages and unknown U+0001 bodies never render", () => {
    expect(threadModel([msg(DANA, "\u0001cbind1:{}"), msg(DANA, "\u0001cdlink1:x"), msg(DANA, "\u0001cfuture9:x")])).toEqual([]);
  });
  it("refs are stable and depend on sender and body", () => {
    expect(messageRef(DANA, "hi")).toBe(messageRef(DANA.toUpperCase().replace("0X", "0x"), "hi"));
    expect(messageRef(DANA, "hi")).not.toBe(messageRef(ME, "hi"));
    expect(messageRef(DANA, "hi")).toMatch(/^m[0-9a-f]{8}$/);
  });
});

describe("nothing a member types can carry a structure line", () => {
  it("plain sends strip control characters, so a pasted Hermes answer does not decode", () => {
    const forged = encodeHermes({ to: "m1a2b3c4d", text: "Trust me", read: ["x"], draft: { title: "Send SALT", budgetSalt: 1, rateSalt: null, workers: null, deadline: null } });
    expect(decodeHermes(forged)).not.toBeNull();
    const sent = plainText(forged);
    expect(sent).not.toMatch(/\u0001/);
    expect(decodeHermes(sent)).toBeNull();
    const t = threadModel([msg(DANA, sent)]);
    expect(t[0].kind).toBe("message");
  });
  it("a typed reply marker does not thread", () => {
    expect(decodeReply(plainText("hi\n\u0001(in reply to m1a2b3c4d)"))).toBeNull();
  });
  it("tab and newline survive", () => {
    expect(plainText("a\tb\nc\u0007")).toBe("a\tb\nc");
  });
});

describe("Hermes answers are readable text, attributed to the device that sent them", () => {
  it("round-trips, with the device taken from the authenticated sender", () => {
    const body = encodeHermes({ to: messageRef(ME, "@Hermes what would that take?"), text: "Five machines reported.", read: ["Group roster and roles"], draft: null });
    expect(body.startsWith(HERMES_HEADER)).toBe(true);
    expect(body).not.toMatch(/[{}"]/);
    const t = threadModel([msg(DANA, body)]);
    expect(t[0].kind).toBe("hermes");
    expect(t[0].kind === "hermes" && t[0].device).toBe(DANA);
  });
  it("nothing in the body can claim another member's device", () => {
    const forged = `${HERMES_HEADER}\non ${ME}'s device\n\nWhat Hermes read: x.\n\u0001(Hermes answer to m00000000)`;
    const t = threadModel([msg(DANA, forged)]);
    expect(t[0].kind === "hermes" && t[0].device).toBe(DANA);
  });
  it("an older app (0.5.3) sees the answer, the draft and what Hermes read as plain words", () => {
    const body = encodeHermes({ to: "m1a2b3c4d", text: "Five machines reported.", read: ["Group roster and roles", "Cluster status: 3 of 6 peers online"], draft: { title: "Tune the support model", budgetSalt: 600, rateSalt: 1.5, workers: 8, deadline: "2026-11-22" } });
    expect(body).toBe(
      [
        HERMES_HEADER,
        "Five machines reported.",
        "",
        "Round draft: Tune the support model (no SALT moved)",
        "Budget: 600 SALT. Rate: 1.5 SALT per lease. Workers: up to 8. Deadline: 2026-11-22.",
        "",
        "What Hermes read: Group roster and roles; Cluster status: 3 of 6 peers online.",
        "\u0001(Hermes answer to m1a2b3c4d)",
      ].join("\n"),
    );
    expect(decodeHermes(body)).toEqual({
      to: "m1a2b3c4d",
      text: "Five machines reported.",
      read: ["Group roster and roles", "Cluster status: 3 of 6 peers online"],
      draft: { title: "Tune the support model", budgetSalt: 600, rateSalt: 1.5, workers: 8, deadline: "2026-11-22" },
    });
  });
  it("a draft with values not proposed round-trips", () => {
    const d = { title: "R", budgetSalt: null, rateSalt: null, workers: null, deadline: null };
    expect(decodeHermes(encodeHermes({ to: "m1a2b3c4d", text: "t", read: [], draft: d }))?.draft).toEqual(d);
  });
  it("drafts are checked: absurd numbers are dropped, a draft needs a title", () => {
    expect(checkDraft({ title: "", budgetSalt: 5 })).toBeNull();
    expect(checkDraft({ title: "Tune", budgetSalt: -1, rateSalt: "1.5", workers: 8.7, deadline: "next week" })).toEqual({
      title: "Tune",
      budgetSalt: null,
      rateSalt: null,
      workers: 8,
      deadline: null,
    });
    expect(decodeHermes(encodeHermes({ to: "m00000000", text: "t", read: [], draft: { title: "R", budgetSalt: 600, rateSalt: null, workers: null, deadline: null } }))?.draft?.budgetSalt).toBe(600);
  });
  it("the group list's last line reads Hermes answers and replies", () => {
    expect(lastLine([msg(DANA, "hello"), msg(DANA, encodeHermes({ to: "m00000000", text: "an answer", read: [], draft: null }))])).toBe("Hermes: an answer");
    expect(lastLine([msg(DANA, "\u0001cbind1:{}")])).toBe("");
  });
});

const ctx = (over: Partial<GroupContext> = {}): GroupContext => ({
  groupName: "Orchard Lab",
  roster: [
    { address: ME, role: "admin" },
    { address: DANA, role: "owner" },
  ],
  cluster: { online: 3, total: 6 },
  rounds: `${ROUNDS_NOT_LIVE}. Both ledgers are deployed, no rounds.`,
  messages: [msg(DANA, "Could we run a round?", 1), msg(ME, "@Hermes what would that take?", 2)],
  asker: ME,
  ...over,
});

describe("Hermes in a group answers only when mentioned, from the group's context alone", () => {
  it("detects @Hermes as a word, any case, and nothing else", () => {
    expect(mentionsHermes("@Hermes what would that take?")).toBe(true);
    expect(mentionsHermes("hey @hermes")).toBe(true);
    expect(mentionsHermes("Hermes, are you there")).toBe(false);
    expect(mentionsHermes("mail me@hermes.example")).toBe(false);
    expect(mentionsHermes("@Hermesbot")).toBe(false);
  });
  it("the request carries group data only and says so", () => {
    const r = buildRequest(ctx());
    const all = r.messagesJson + r.contextJson;
    expect(all).toContain("Orchard Lab");
    expect(all).toContain(ROUNDS_NOT_LIVE);
    expect(all).not.toMatch(/wallet"\s*:|memory"\s*:|balance/i);
    expect(JSON.parse(r.messagesJson)[0].content).toContain("posted to every member");
    expect(JSON.parse(r.messagesJson)[0].content).toContain("Never mention USD, earnings");
  });
  it("what Hermes read is listed, and an unread cluster says so", () => {
    expect(readList(ctx())).toContain("Cluster status: 3 of 6 peers online");
    expect(readList(ctx({ cluster: null }))).toContain("Cluster status: not read (the cluster did not answer)");
    expect(NOT_READ).toContain("private chats");
  });
  it("a round-draft block becomes a checked draft and leaves the text", () => {
    const p = parseAnswer('Three machines have 24 GB.\n```round-draft\n{"title":"Tune the support model","budgetSalt":600,"rateSalt":1.5,"workers":8}\n```');
    expect(p.text).toBe("Three machines have 24 GB.");
    expect(p.draft).toEqual({ title: "Tune the support model", budgetSalt: 600, rateSalt: 1.5, workers: 8, deadline: null });
  });
  const passScan = async (t: string): Promise<DisclosureScan> => ({ scanner: "citrate-pii-scan/1", passed: true, found: [], redacted: t, coverage: "c" });
  const post = async (p: SendPreview): Promise<SendDecision> => ({ post: true, text: p.text });
  const deps = (over: Partial<HermesDeps>): HermesDeps => ({ inferLocal: async () => "ok", scan: passScan, confirm: post, send: vi.fn(async () => {}), ...over });

  it("posts the answer to the group as a Hermes answer message once the member confirms", async () => {
    const send = vi.fn(async () => {});
    const confirm = vi.fn(post);
    const out = await answerInGroup("g", "m12345678", ctx(), deps({ inferLocal: async () => "Five machines reported.", send, confirm }));
    expect(confirm).toHaveBeenCalledTimes(1);
    expect(confirm.mock.calls[0][0]).toMatchObject({ groupId: "g", text: "Five machines reported." });
    expect(out.kind).toBe("posted");
    const body = send.mock.calls[0][1] as string;
    expect(decodeHermes(body)).toMatchObject({ to: "m12345678", text: "Five machines reported." });
  });
  it("when the local model does not answer, nothing is posted", async () => {
    const send = vi.fn(async () => {});
    const out = await answerInGroup("g", "m12345678", ctx(), deps({ inferLocal: async () => Promise.reject(new Error("local model not running")), send }));
    expect(out).toEqual({ kind: "no-answer", reason: "local model not running" });
    expect(send).not.toHaveBeenCalled();
  });
  it("an empty answer posts nothing; a refused send says it was not sent", async () => {
    expect((await answerInGroup("g", "m12345678", ctx(), deps({ inferLocal: async () => "  " }))).kind).toBe("no-answer");
    const out = await answerInGroup("g", "m12345678", ctx(), deps({ send: async () => Promise.reject(new Error("relay down")) }));
    expect(out.kind).toBe("not-sent");
  });
  it("V6: a declined card, a failed check or a finding in the final text posts nothing", async () => {
    const send = vi.fn(async () => {});
    expect(await answerInGroup("g", "m1", ctx(), deps({ send, confirm: async () => ({ post: false }) }))).toEqual({ kind: "held", reason: "you chose not to post it" });
    expect((await answerInGroup("g", "m1", ctx(), deps({ send, confirm: async () => Promise.reject(new Error("closed")) }))).kind).toBe("held");
    const down = await answerInGroup("g", "m1", ctx(), deps({ send, scan: async () => Promise.reject(new Error("no scanner")) }));
    expect(down).toMatchObject({ kind: "held" });
    expect((down as { reason: string }).reason).toContain("could not run (no scanner)");
    // The member's chosen text is scanned again, whatever the card showed.
    const found: DisclosureScan = { scanner: "s", passed: false, found: [{ class: "email", label: "email address", count: 1 }], redacted: "<email>", coverage: "c" };
    const edited = await answerInGroup("g", "m1", ctx(), deps({ send, confirm: async () => ({ post: true, text: "mail me at x" }), scan: async (t) => (t.includes("mail me") ? found : passScan(t)) }));
    expect(edited).toEqual({ kind: "held", reason: "the text still holds email address, so nothing was posted" });
    expect(send).not.toHaveBeenCalled();
  });
  it("V6 (Codex review of #730): the exact body that is posted is scanned, round draft included", async () => {
    const mail = ["dana", "@", "example.org"].concat();
    const flag = async (t: string): Promise<DisclosureScan> =>
      t.includes(mail) ? { scanner: "s", passed: false, found: [{ class: "email", label: "email address", count: 1 }], redacted: t.split(mail).join("<email>"), coverage: "c" } : passScan(t);
    const scanned: string[] = [];
    const scan = async (t: string) => (scanned.push(t), flag(t));
    const send = vi.fn(async () => {});
    const confirm = vi.fn(post);
    const raw = 'Five machines reported.\n```round-draft\n{"title":"Pay ' + mail + '","budgetSalt":600}\n```';
    const out = await answerInGroup("g", "m1", ctx(), deps({ inferLocal: async () => raw, scan, send, confirm }));
    expect(out).toMatchObject({ kind: "held" });
    expect((out as { reason: string }).reason).toContain("round draft");
    expect(send).not.toHaveBeenCalled();
    expect(confirm).not.toHaveBeenCalled();
    // A clean draft: what is sent is byte for byte a text that was scanned.
    const clean = 'Five machines reported.\n```round-draft\n{"title":"Tune the support model","budgetSalt":600}\n```';
    scanned.length = 0;
    const ok = await answerInGroup("g", "m1", ctx(), deps({ inferLocal: async () => clean, scan, send, confirm }));
    expect(ok.kind).toBe("posted");
    expect(scanned).toContain(send.mock.calls[0][1]);
    expect(confirm.mock.calls[0][0].draftLines?.[0]).toContain("Tune the support model");
  });
  it("V6: the card says how many other members receive the answer", () => {
    expect(recipientsOf(ctx())).toBe(ctx().roster.filter((r) => r.address.toLowerCase() !== ctx().asker.toLowerCase()).length);
    expect(recipientsOf(ctx({ roster: [] }))).toBeNull();
  });
});

describe("round words (UI truth contract v1.2, #676), from Lane 1's FL network state", () => {
  const ZERO_UNITS = "0x" + "0".repeat(64);
  const reads = (over: Partial<FlReads> = {}): FlReads => ({ codeSize: async () => 1234, viewCall: async () => ZERO_UNITS, ...over });
  it("today's book: Group rounds not live yet, both ledgers deployed, no rounds", async () => {
    const t = roundsToday(await readFlNetworkState(realBook, reads()));
    expect(t).toEqual({ kind: "not-live", detail: "Both ledgers are deployed, no rounds." });
    expect(roundsLine(t)).toBe("Group rounds not live yet. Both ledgers are deployed, no rounds.");
    expect(roundsLine(t)).not.toMatch(/dormant/i);
  });
  it("a failed read the state depends on is — with the reason, never zero rounds", async () => {
    const t = roundsToday(await readFlNetworkState({ addresses: { ...realBook.addresses, WorkMarket: "0x" + "ab".repeat(20) } }, reads({ codeSize: async () => Promise.reject(new Error("rpc timeout")) })));
    expect(t.kind).toBe("unknown");
    expect(roundsLine(t)).toMatch(/^— \(the ledger read failed: .*rpc timeout\)$/);
  });
  it("before the reads answer, the chat says it is reading", () => {
    expect(roundsToday(bookOnlyState({ addresses: { ...realBook.addresses, WorkMarket: "0x" + "ab".repeat(20) } })).kind).toBe("reading");
  });
  it("with WorkMarket and FRL deployed (0.6.0, fixture), rounds can be offered", async () => {
    const t = roundsToday(await readFlNetworkState({ addresses: { ...realBook.addresses, WorkMarket: "0x" + "ab".repeat(20) } }, reads()));
    expect(t).toEqual({ kind: "live" });
  });
  it("no live round exists on this network (WorkMarket is not deployed)", () => {
    expect(liveRoundsFor("g")).toEqual([]);
  });
  it("a live round card's sentence (0.6.0, fixture)", () => {
    expect(liveRoundSentence({ round: 14, stage: "Verifying", leases: 8, verified: 6, windowEnds: "in about 3 h 10 min", yourLease: "verified" })).toBe(
      "Round 14: verifying, 6 of 8 leases verified, challenge window ends in about 3 h 10 min, your lease verified",
    );
  });
  it("drafts by role: owners and admins review, members wait, read-only reads", () => {
    expect(draftActions("owner")).toBe("review");
    expect(draftActions("admin")).toBe("review");
    expect(draftActions("member")).toBe("wait");
    expect(draftActions(null)).toBe("wait");
    expect(draftActions("guest")).toBe("read-only");
  });
});

describe("alerts point at the screen where you act", () => {
  const draftBody = encodeHermes({ to: messageRef(ME, "@Hermes draft it"), text: "Here is a draft.", read: [], draft: { title: "Support tickets", budgetSalt: 600, rateSalt: null, workers: null, deadline: null } });
  const history = { g: [msg(ME, "@Hermes draft it", 1), msg(DANA, draftBody, 2)] };
  it("a draft alerts owners and admins only, and Hermes answers to your message alert you, both marked not verified", () => {
    const asAdmin = groupAlerts({ groups: [{ id: "g", name: "Orchard Lab" }], history, roles: { g: "admin" }, requests: {}, me: ME });
    expect(asAdmin.map((a) => a.kind).sort()).toEqual(["answer", "draft"]);
    const d = asAdmin.find((a) => a.kind === "draft")!;
    expect(d.tag).toBe("Hermes");
    expect(d.text).toContain("(not verified)");
    expect(asAdmin.find((a) => a.kind === "answer")?.text).toContain("(not verified)");
    const asMember = groupAlerts({ groups: [{ id: "g", name: "Orchard Lab" }], history, roles: { g: "member" }, requests: {}, me: ME });
    expect(asMember.map((a) => a.kind)).toEqual(["answer"]);
  });
  it("only a draft this device's Hermes posted gets the HIC-1 review alert", () => {
    const own = encodeHermes({ to: "m00000000", text: "d", read: [], draft: { title: "Mine", budgetSalt: null, rateSalt: null, workers: null, deadline: null } });
    const a = groupAlerts({ groups: [{ id: "g", name: "Orchard Lab" }], history: { g: [msg(ME, own)] }, roles: { g: "owner" }, requests: {}, me: ME, ownAnswers: new Set([messageRef(ME, own)]) });
    expect(a).toEqual([expect.objectContaining({ kind: "draft", tag: "HIC-1" })]);
  });
  it("invite requests open Members", () => {
    const a = groupAlerts({ groups: [{ id: "g", name: "Orchard Lab" }], history: {}, roles: { g: "owner" }, requests: { g: [{ inviteId: "i1", group: "g", address: DANA, inviteLabel: "Ana", approving: false }] }, me: ME });
    expect(a).toEqual([expect.objectContaining({ kind: "invite", route: "groups/members" })]);
  });
  it("nothing to alert is an empty list", () => {
    expect(groupAlerts({ groups: [], history: {}, roles: {}, requests: {}, me: ME })).toEqual([]);
  });
});
