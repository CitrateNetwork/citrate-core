// 0.5.4 FL design, board 02 (Groups › Chat). One test per StateMatrix "Group chat" state: Empty (no
// groups yet), Not deployed (draft card: Group rounds not live yet), Pending (pending invite),
// Running (a live round card, 0.6.0 fixture only), Success (joined, in the list), Failure (we
// couldn't find that invite), Unavailable (presence and capacity "—"). Plus Hermes on mention with
// its disclosure line, replies, read-only, alerts and the create sheet.
import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { renderToStaticMarkup } from "react-dom/server";
import { GroupChat, EPOCH_UNKNOWN, HERMES_DISCLOSURE } from "./GroupChat";
import { RoundEventCard, RoundDraftCard, type RoundsToday } from "./RoundCards";
import { encodeHermes, messageRef, decodeHermes, encodeReply } from "./groupEnvelope";
import { UNKNOWN_INVITE } from "./GroupSheets";
import { rememberOwnAnswer } from "./groupHermes";
import { bridge } from "../../bridge";
import { freshState, type AppState } from "../../shell/state";
import { groupsSlice } from "../../shell/slices/groups";
import type { Store } from "../../shell/store";
import type { Group, GroupMember, GroupMessage, MyInviteRequest } from "../../bridge/domains";
import type { DisclosureScan } from "./groupHermes";

// V6: the on-device private-information check is Rust (`pii_scan_text`); the board's tests drive
// its result through this seam.
const scanMock = vi.hoisted(() => ({ fn: null as null | ((t: string) => Promise<DisclosureScan>) }));
vi.mock("../../privacy/disclosureScan", () => ({
  SCAN_NEEDS_DESKTOP: "the private-information check runs only in the desktop app",
  scanForDisclosure: (t: string) => (scanMock.fn ? scanMock.fn(t) : Promise.reject(new Error("no scanner in this test"))),
}));
const COVERAGE = "Checked for email addresses and more.";
const clean = (t: string): DisclosureScan => ({ scanner: "citrate-pii-scan/1", passed: true, found: [], redacted: t, coverage: COVERAGE });

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const ME = "0x1111111111111111111111111111111111111111";
const DANA = "0x2222222222222222222222222222222222222222";
const ROSTER: GroupMember[] = [
  { address: DANA, role: "owner" },
  { address: ME, role: "admin" },
];
const GROUP: Group = { id: "g-chat", name: "Orchard Lab", owner: DANA, kind: "forum", members: ROSTER };
const TODAY: RoundsToday = { kind: "not-live", detail: "Both ledgers are deployed, no rounds." };

let host: HTMLDivElement;
let root: Root;
let toast: ReturnType<typeof vi.fn>;
let go: ReturnType<typeof vi.fn>;

const store = (): Store =>
  ({
    toast,
    go,
    identity: () => ({ wallet: "", name: "Me" }),
    refreshPeople: async () => {},
    requestSig: vi.fn(),
    canJoinNow: () => true,
    openInviteLink: () => {},
    clearInviteLink: () => {},
  }) as unknown as Store;
const state = (over: Partial<AppState> = {}): AppState => ({ ...freshState("p1"), commsAddr: ME, ...over });

function seed(messages: GroupMessage[] = [], roster: GroupMember[] = ROSTER) {
  vi.spyOn(bridge.groups, "list").mockResolvedValue([GROUP]);
  vi.spyOn(bridge.groups, "roster").mockResolvedValue(roster);
  vi.spyOn(bridge.groups, "messages").mockResolvedValue([]);
  groupsSlice.set({ groups: [GROUP], selectedId: GROUP.id, roster, messages, history: { [GROUP.id]: messages }, names: {}, error: null, sending: false });
}

beforeEach(() => {
  vi.restoreAllMocks();
  localStorage.clear();
  scanMock.fn = async (t) => clean(t);
  vi.spyOn(bridge.groups, "selfAddress").mockResolvedValue(ME);
  vi.spyOn(bridge.social, "resolve").mockResolvedValue([]);
  vi.spyOn(bridge.cluster, "devices").mockRejectedValue(new Error("cluster daemon not running"));
  vi.spyOn(bridge.cluster, "status").mockRejectedValue(new Error("cluster daemon not running"));
  vi.spyOn(bridge.invites, "myRequests").mockResolvedValue([]);
  vi.spyOn(bridge.invites, "requests").mockResolvedValue([]);
  vi.spyOn(bridge.contracts, "codeSize").mockResolvedValue(100);
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
  toast = vi.fn();
  go = vi.fn();
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  groupsSlice.set({ groups: [], selectedId: null, roster: [], messages: [], history: {}, names: {}, error: null });
});

async function render(over: Partial<AppState> = {}, view: "groups" | "alerts" = "groups", rounds: RoundsToday = TODAY) {
  await act(async () => root.render(<GroupChat store={store()} s={state(over)} view={view} rounds={rounds} />));
}
const board = () => host.querySelector("[data-board=group-chat]") as HTMLElement;
const btn = (label: string | RegExp) =>
  [...host.querySelectorAll("button")].find((b) => (typeof label === "string" ? b.textContent === label : label.test(b.textContent ?? ""))) as HTMLButtonElement | undefined;
const setValue = async (el: HTMLTextAreaElement | HTMLInputElement, v: string) => {
  const proto = el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
  Object.getOwnPropertyDescriptor(proto, "value")!.set!.call(el, v);
  await act(async () => el.dispatchEvent(new Event("input", { bubbles: true })));
};

describe("Groups › Chat states (StateMatrix)", () => {
  it("Empty: no groups yet, with create and join", async () => {
    vi.spyOn(bridge.groups, "list").mockResolvedValue([]);
    await render();
    expect(board().dataset.state).toBe("empty");
    expect(host.querySelector("h1")?.textContent).toBe("No groups yet");
    expect(btn("Create a group")).toBeDefined();
    expect(btn("Join with an invite")).toBeDefined();
  });

  it("Not deployed: a Hermes round draft reads Group rounds not live yet and offers no join", async () => {
    const ask = "@Hermes what would that take for us?";
    const body = encodeHermes({ to: messageRef(ME, ask), text: "Five machines reported.", read: ["Group roster and roles"], draft: { title: "tune the support model", budgetSalt: 600, rateSalt: 1.5, workers: 8, deadline: "2026-11-22" } });
    // This device's Hermes posted it, so it is verified and an admin may review it.
    rememberOwnAnswer(messageRef(ME, body));
    seed([{ id: "1", groupId: GROUP.id, sender: ME, body: ask, ts: 1 }, { id: "2", groupId: GROUP.id, sender: ME, body, ts: 2 }]);
    await render();
    const card = host.querySelector("[data-testid=round-draft-card]") as HTMLElement;
    expect(card.textContent).toContain("Group rounds not live yet");
    expect(card.textContent).toContain("Draft · no SALT moved");
    expect(card.textContent).toContain("600 SALT");
    expect(card.textContent).toContain("budget covers 400 leases");
    expect(card.textContent).toContain("Hermes cannot press these buttons");
    expect([...card.querySelectorAll("button")].map((b) => b.textContent)).toEqual(["Review draft"]);
    await act(async () => card.querySelector("button")!.click());
    await act(async () => {});
    expect(go).toHaveBeenCalledWith("groups/rounds/open");
    expect(host.textContent).not.toMatch(/\bJoin round\b|Deployed · dormant/);
    expect(host.querySelector("[data-testid=rail-rounds]")?.textContent).toContain("Group rounds not live yet. Both ledgers are deployed, no rounds.");
  });

  it("Pending: a pending invite this device sent shows in the list", async () => {
    seed();
    const r: MyInviteRequest = { id: "r1", group: "g2", groupName: "Field Robotics", inviter: "Dana", state: "waiting", expiresAt: 4_000_000_000, note: "" } as MyInviteRequest;
    vi.spyOn(bridge.invites, "myRequests").mockResolvedValue([r]);
    await render();
    expect(host.querySelector("[data-testid=invite-request]")?.getAttribute("data-state")).toBe("waiting");
    expect(host.textContent).toContain("Field Robotics");
  });

  it("Running (0.6.0, fixture only): a live round card names its state in one sentence and opens the live round", async () => {
    const open = vi.fn();
    await act(async () => root.render(<RoundEventCard r={{ round: 14, stage: "Verifying", leases: 8, verified: 6, windowEnds: "in about 3 h 10 min", yourLease: "verified" }} onOpen={open} />));
    const b = host.querySelector("[data-testid=round-event-card]") as HTMLButtonElement;
    expect(b.getAttribute("aria-label")).toBe("Round 14: verifying, 6 of 8 leases verified, challenge window ends in about 3 h 10 min, your lease verified. Open the live round.");
    await act(async () => b.click());
    expect(open).toHaveBeenCalled();
    expect(b.textContent).not.toMatch(/Join/);
  });

  it("Success: a joined group is in the list", async () => {
    seed();
    await render();
    const nav = host.querySelector("nav[aria-label=Groups]") as HTMLElement;
    expect(nav.textContent).toContain("Orchard Lab");
    expect(nav.querySelector("[aria-current=page]")?.textContent).toContain("Orchard Lab");
  });

  it("Failure: an invite that is not a link reads We couldn't find that invite, never expired", async () => {
    seed();
    await render();
    await act(async () => btn("Join with an invite")!.click());
    const input = host.querySelector("#grp-invite") as HTMLInputElement;
    await setValue(input, "7KQ2M9XD");
    await act(async () => btn("Check invite")!.click());
    const alert = [...host.querySelectorAll("[role=alert]")].map((a) => a.textContent);
    expect(alert).toContain(UNKNOWN_INVITE);
    expect(UNKNOWN_INVITE).not.toMatch(/expired/i);
  });

  it("Unavailable: presence, peers and the epoch are — with the reason, never zero", async () => {
    seed();
    await render();
    expect(host.textContent).toContain("2 members · online —");
    expect(host.querySelector("[data-testid=group-epoch]")?.getAttribute("title")).toBe(EPOCH_UNKNOWN);
    const rail = host.querySelector("aside[aria-label='Group details']") as HTMLElement;
    expect(rail.textContent).toContain("the cluster did not answer");
    expect(rail.textContent).not.toMatch(/\b0\/0\b/);
  });
});

describe("Hermes answers only when mentioned, on your device, and says so", () => {
  it("a message without @Hermes runs no model", async () => {
    seed();
    const infer = vi.spyOn(bridge.chat, "inferLocal").mockResolvedValue("x");
    const send = vi.spyOn(bridge.groups, "send").mockResolvedValue();
    await render();
    await setValue(host.querySelector("#grp-msg") as HTMLTextAreaElement, "hello all");
    await act(async () => btn("Send")!.click());
    expect(send).toHaveBeenCalledWith(GROUP.id, "hello all");
    expect(infer).not.toHaveBeenCalled();
  });

  it("@Hermes runs the local model, shows the send card, and posts only when the member presses Post (V6)", async () => {
    seed();
    vi.spyOn(bridge.chat, "inferLocal").mockResolvedValue("Five machines reported hardware.");
    const send = vi.spyOn(bridge.groups, "send").mockResolvedValue();
    await render();
    expect(host.textContent).toContain(HERMES_DISCLOSURE);
    await setValue(host.querySelector("#grp-msg") as HTMLTextAreaElement, "@Hermes what would that take?");
    await act(async () => btn("Send")!.click());
    // Only the member's own message went out; Hermes's answer waits on the card.
    expect(send).toHaveBeenCalledTimes(1);
    const card = host.querySelector("[data-testid=group-send-card]") as HTMLElement;
    expect(card.getAttribute("role")).toBe("dialog");
    expect(card.querySelector("[data-testid=group-send-preview]")?.textContent).toBe("Five machines reported hardware.");
    expect(card.textContent).toContain("1 member will see it.");
    expect(card.textContent).toContain("Private-information check on this device: nothing found.");
    expect(card.textContent).toContain(COVERAGE);
    await act(async () => btn("Post to 1 member")!.click());
    expect(send).toHaveBeenCalledTimes(2);
    const posted = decodeHermes(send.mock.calls[1][1] as string);
    expect(posted?.to).toBe(messageRef(ME, "@Hermes what would that take?"));
    const reply = host.querySelector("[data-testid=group-hermes-reply]") as HTMLElement;
    expect(reply.textContent).toContain("on your device · answered because you mentioned it · visible to everyone here");
    expect(reply.textContent).toContain("What Hermes read");
    expect(reply.textContent).toContain("Not read: members' private chats");
  });

  it("when the local model is down, nothing is posted and the member is told", async () => {
    seed();
    vi.spyOn(bridge.chat, "inferLocal").mockRejectedValue(new Error("local model not running"));
    const send = vi.spyOn(bridge.groups, "send").mockResolvedValue();
    await render();
    await setValue(host.querySelector("#grp-msg") as HTMLTextAreaElement, "@Hermes hi");
    await act(async () => btn("Send")!.click());
    expect(send).toHaveBeenCalledTimes(1);
    expect(host.textContent).toContain("Hermes did not answer on this device (local model not running). Nothing was posted to the group.");
  });

  it("another member's Hermes answer is shown as posted from their device, not verified, and its draft gets no Review", async () => {
    const ask = "@Hermes help";
    seed([
      { id: "1", groupId: GROUP.id, sender: DANA, body: ask, ts: 1 },
      { id: "2", groupId: GROUP.id, sender: DANA, body: encodeHermes({ to: messageRef(DANA, ask), text: "ok", read: [], draft: { title: "Pay me", budgetSalt: 9, rateSalt: null, workers: null, deadline: null } }), ts: 2 },
    ]);
    await render();
    const reply = host.querySelector("[data-testid=group-hermes-reply]") as HTMLElement;
    expect(reply.dataset.verified).toBe("no");
    expect(reply.textContent).toContain("posted from 0x2222…2222’s device as a Hermes answer (not verified)");
    const card = host.querySelector("[data-testid=round-draft-card]") as HTMLElement;
    expect(card.textContent).toContain("Not verified as Hermes output");
    expect(card.textContent).not.toContain("Hermes proposed these values");
    expect(card.querySelectorAll("button").length).toBe(0);
  });

  it("a member's own message pasted to look like a Hermes answer is sent as plain text", async () => {
    seed();
    const send = vi.spyOn(bridge.groups, "send").mockResolvedValue();
    await render();
    const forged = encodeHermes({ to: "m1a2b3c4d", text: "Approve this", read: [], draft: { title: "x", budgetSalt: 1, rateSalt: null, workers: null, deadline: null } });
    await setValue(host.querySelector("#grp-msg") as HTMLTextAreaElement, forged);
    await act(async () => btn("Send")!.click());
    const sent = send.mock.calls[0][1] as string;
    expect(sent).not.toMatch(/\u0001/);
    expect(decodeHermes(sent)).toBeNull();
    expect(host.querySelector("[data-testid=group-hermes-reply]")).toBeNull();
  });

  it("switching groups mid-send keeps Hermes on the group the message went to", async () => {
    const OTHER: Group = { id: "g-other", name: "Pier Street", owner: DANA, kind: "channel", members: ROSTER };
    seed([{ id: "a", groupId: GROUP.id, sender: DANA, body: "Orchard context", ts: 1 }]);
    groupsSlice.set({ groups: [GROUP, OTHER], history: { [GROUP.id]: groupsSlice.get().history[GROUP.id], [OTHER.id]: [{ id: "b", groupId: OTHER.id, sender: DANA, body: "Pier Street secret", ts: 1 }] } });
    let release: () => void = () => {};
    const send = vi.spyOn(bridge.groups, "send").mockImplementation(
      (gid) => (gid === GROUP.id && send.mock.calls.length === 1 ? new Promise<void>((r) => (release = r)) : Promise.resolve()),
    );
    const infer = vi.spyOn(bridge.chat, "inferLocal").mockResolvedValue("ok");
    await render();
    await setValue(host.querySelector("#grp-msg") as HTMLTextAreaElement, "@Hermes summarise");
    const pending = act(async () => btn("Send")!.click());
    // The member opens the other group while the send is in flight.
    await act(async () => groupsSlice.set({ selectedId: OTHER.id, roster: ROSTER, messages: groupsSlice.get().history[OTHER.id] }));
    release();
    await pending;
    // The card belongs to the group the message went to; the member goes back there to post it.
    expect(host.querySelector("[data-testid=group-send-card]")).toBeNull();
    await act(async () => groupsSlice.set({ selectedId: GROUP.id, roster: ROSTER, messages: groupsSlice.get().history[GROUP.id] }));
    await act(async () => btn("Post to 1 member")!.click());
    const [messagesJson, contextJson] = infer.mock.calls[0];
    expect(messagesJson).toContain("Orchard context");
    expect(messagesJson + contextJson).not.toContain("Pier Street");
    expect(send.mock.calls.map((c) => c[0])).toEqual([GROUP.id, GROUP.id]);
  });
});

describe("V6: the @Hermes send card and the private-information check", () => {
  const ask = async () => {
    await setValue(host.querySelector("#grp-msg") as HTMLTextAreaElement, "@Hermes who should we ask?");
    await act(async () => btn("Send")!.click());
  };

  it("a finding blocks posting the answer as written; the member can post the redaction", async () => {
    seed();
    const mail = ["dana", "@", "example.org"].concat();
    vi.spyOn(bridge.chat, "inferLocal").mockResolvedValue(`Write to ${mail}.`);
    scanMock.fn = async (t) =>
      t.includes(mail)
        ? { scanner: "citrate-pii-scan/1", passed: false, found: [{ class: "email", label: "email address", count: 1 }], redacted: "Write to <email>.", coverage: COVERAGE }
        : clean(t);
    const send = vi.spyOn(bridge.groups, "send").mockResolvedValue();
    await render();
    await ask();
    const card = host.querySelector("[data-testid=group-send-card]") as HTMLElement;
    expect(card.querySelector("[role=alert]")?.textContent).toContain("found 1 email address");
    expect(btn("Post to 1 member")).toBeUndefined();
    await act(async () => btn("Post with these removed")!.click());
    expect(send).toHaveBeenCalledTimes(2);
    const posted = decodeHermes(send.mock.calls[1][1] as string);
    expect(posted?.text).toBe("Write to <email>.");
    expect(send.mock.calls[1][1]).not.toContain(mail);
  });

  it("an edit that still holds private information is checked again and not posted", async () => {
    seed();
    const phone = ["202", "-555-", "0143"].concat();
    vi.spyOn(bridge.chat, "inferLocal").mockResolvedValue("Ask the owners.");
    scanMock.fn = async (t) =>
      t.includes(phone)
        ? { scanner: "citrate-pii-scan/1", passed: false, found: [{ class: "phone", label: "phone number", count: 1 }], redacted: t.replace(phone, "<phone>"), coverage: COVERAGE }
        : clean(t);
    const send = vi.spyOn(bridge.groups, "send").mockResolvedValue();
    await render();
    await ask();
    await act(async () => btn("Edit")!.click());
    await setValue(host.querySelector("#grp-send-edit") as HTMLTextAreaElement, `Ask the owners, or call ${phone}.`);
    await act(async () => btn("Post to 1 member")!.click());
    expect(send).toHaveBeenCalledTimes(1);
    expect(host.querySelector("[role=alert]")?.textContent).toContain("still holds phone number, so nothing was posted");
  });

  it("Don't post posts nothing and closes the card", async () => {
    seed();
    vi.spyOn(bridge.chat, "inferLocal").mockResolvedValue("ok");
    const send = vi.spyOn(bridge.groups, "send").mockResolvedValue();
    await render();
    await ask();
    await act(async () => btn("Don't post")!.click());
    expect(send).toHaveBeenCalledTimes(1);
    expect(host.querySelector("[data-testid=group-send-card]")).toBeNull();
    expect(host.querySelector("[role=alert]")).toBeNull();
  });

  it("when the check cannot run, nothing is posted and the member is told why", async () => {
    seed();
    vi.spyOn(bridge.chat, "inferLocal").mockResolvedValue("ok");
    scanMock.fn = async () => Promise.reject(new Error("the private-information check runs only in the desktop app"));
    const send = vi.spyOn(bridge.groups, "send").mockResolvedValue();
    await render();
    await ask();
    expect(send).toHaveBeenCalledTimes(1);
    expect(host.querySelector("[data-testid=group-send-card]")).toBeNull();
    expect(host.textContent).toContain("Hermes's answer was not posted: the private-information check could not run (the private-information check runs only in the desktop app), so nothing was posted.");
  });
});

describe("threads, roles and the composer", () => {
  it("replies collapse under their message and a reply is sent as a reply message", async () => {
    const parent = "Could we run a round this week?";
    seed([
      { id: "1", groupId: GROUP.id, sender: DANA, body: parent, ts: 1 },
      { id: "2", groupId: GROUP.id, sender: DANA, body: encodeReply(messageRef(DANA, parent), "Mine is on battery most nights."), ts: 2 },
    ]);
    const send = vi.spyOn(bridge.groups, "send").mockResolvedValue();
    await render();
    const toggle = btn("1 reply")!;
    expect(toggle.getAttribute("aria-expanded")).toBe("false");
    await act(async () => toggle.click());
    expect(host.textContent).toContain("Mine is on battery most nights.");
    await act(async () => btn("Reply")!.click());
    await setValue(host.querySelector(`#reply-${messageRef(DANA, parent)}`) as HTMLInputElement, "I can lend my desktop.");
    await act(async () => btn("Send reply")!.click());
    expect(send).toHaveBeenCalledWith(GROUP.id, encodeReply(messageRef(DANA, parent), "I can lend my desktop."));
  });

  it("read-only members cannot post", async () => {
    seed([], [{ address: DANA, role: "owner" }, { address: ME, role: "guest" }]);
    await render();
    const ta = host.querySelector("#grp-msg") as HTMLTextAreaElement;
    expect(ta.disabled).toBe(true);
    expect(ta.placeholder).toBe("Read-only members cannot post here");
    expect(btn("Send")!.disabled).toBe(true);
  });

  it("a draft shows each role what it may do", () => {
    const d = { title: "t", budgetSalt: null, rateSalt: null, workers: null, deadline: null };
    const html = (role: "member" | "guest" | "owner") => renderToStaticMarkup(<RoundDraftCard draft={d} rounds={TODAY} myRole={role} verified onReview={() => {}} />);
    expect(html("member")).toContain("Waiting for an owner or admin to review it.");
    expect(html("guest")).toContain("Read-only members can see drafts but not act on them.");
    expect(html("owner")).toContain("Review draft");
    expect(html("owner")).toContain("not proposed");
    expect(html("owner")).toContain('data-unknown="true"');
  });

  it("an unknown round ledger read shows — on the draft, not live or zero", () => {
    const d = { title: "t", budgetSalt: 1, rateSalt: null, workers: null, deadline: null };
    const h = renderToStaticMarkup(<RoundDraftCard draft={d} rounds={{ kind: "unknown", reason: "rpc timeout" }} myRole="owner" verified onReview={() => {}} />);
    expect(h).toContain("Rounds: —");
    expect(h).toContain("rpc timeout");
  });

  it("create a group sheet: Cancel is the safe default, Escape closes", async () => {
    seed();
    await render();
    await act(async () => btn("Create a group")!.click());
    const dialog = host.querySelector("[role=dialog]") as HTMLElement;
    expect(dialog.getAttribute("aria-modal")).toBe("true");
    expect(document.activeElement?.textContent).toBe("Cancel");
    expect(dialog.textContent).toContain("You are the owner.");
    await act(async () => dialog.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true })));
    expect(host.querySelector("[role=dialog]")).toBeNull();
  });
});

describe("Alerts", () => {
  it("an alert opens the screen where you act and carries no approve button", async () => {
    const ask = "@Hermes draft it";
    seed([
      { id: "1", groupId: GROUP.id, sender: ME, body: ask, ts: 1 },
      { id: "2", groupId: GROUP.id, sender: DANA, body: encodeHermes({ to: messageRef(ME, ask), text: "draft", read: [], draft: { title: "Support tickets", budgetSalt: null, rateSalt: null, workers: null, deadline: null } }), ts: 2 },
    ]);
    await render({ myGroups: [{ id: GROUP.id, name: "Orchard Lab", kind: "forum", myRole: "admin", iManage: true }] }, "alerts");
    expect(host.textContent).toContain("(not verified)");
    expect(board().dataset.state).toBe("alerts");
    const items = [...host.querySelectorAll("[data-alert]")];
    expect(items.map((i) => i.getAttribute("data-alert")).sort()).toEqual(["answer", "draft"]);
    expect(host.textContent).not.toMatch(/\bApprove\b/);
    expect(host.textContent).toContain("They never carry an approve button.");
    await act(async () => (items[0] as HTMLButtonElement).click());
    expect(go).toHaveBeenCalledWith("groups/chat");
  });

  it("no alerts is an honest empty line", async () => {
    seed();
    await render({}, "alerts");
    expect(host.querySelector("[data-testid=alerts-empty]")).not.toBeNull();
  });
});

describe("copy rules", () => {
  it("no exclamation marks, members not users, no banned words", async () => {
    seed();
    await render();
    const t = host.textContent ?? "";
    expect(t).not.toMatch(/!/);
    expect(t).not.toMatch(/\busers?\b/i);
    expect(t).not.toMatch(/\b(earn|earnings|income|yield|APY|USD|HITL)\b|Deployed · dormant/i);
  });
});
