// 0.6.0 L4-WP01: characterization suite for group conversations (Groups › Chat).
//
// Every assertion here describes how group chat behaves TODAY (citrate-core main a826e441) and
// passes on day 1. These are not red tests and not a statement of what is right: they pin current
// behaviour so the later L4 work packages change it on purpose. LQ runs this file on every merge;
// a later WP may replace an assertion here only with a stricter one, and says so in its PR.
//
// Data source (Rule 7): the shipped components and their existing seams. The group list, roster
// and messages come from the comms-member-daemon through `bridge.groups` (spied here, as in
// groupChat.test.tsx); the Tauri groups bridge is driven through a mocked `invoke`.
import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { GroupChat, HERMES_DISCLOSURE } from "./GroupChat";
import { type RoundsToday } from "./RoundCards";
import { decodeHermes, encodeReply, lastLine, messageRef, threadModel } from "./groupEnvelope";
import { bridge } from "../../bridge";
import { tauriGroups } from "../../bridge/tauri/comms";
import { DEVICE_LINKS_MSG_PREFIX, SOCIAL_BINDING_MSG_PREFIX } from "../../bridge/domains";
import { freshState, type AppState } from "../../shell/state";
import { groupsSlice, reloadMessages } from "../../shell/slices/groups";
import type { Store } from "../../shell/store";
import type { Group, GroupMember, GroupMessage } from "../../bridge/domains";
import { buildRequest, type DisclosureScan } from "./groupHermes";

// The on-device private-information check is Rust (`pii_scan_text`); as in groupChat.test.tsx its
// result is driven through this seam.
const scanMock = vi.hoisted(() => ({ fn: null as null | ((t: string) => Promise<DisclosureScan>) }));
vi.mock("../../privacy/disclosureScan", () => ({
  SCAN_NEEDS_DESKTOP: "the private-information check runs only in the desktop app",
  scanForDisclosure: (t: string) => (scanMock.fn ? scanMock.fn(t) : Promise.reject(new Error("no scanner in this test"))),
}));

// The Tauri groups bridge (`bridge/tauri/comms.ts`) calls the app's `invoke`; here it is a spy. With
// no handler set it rejects the way a missing Tauri runtime does.
const invokeMock = vi.hoisted(() => vi.fn());
vi.mock("../../bridge/tauri/invoke", async (orig) => ({
  ...(await orig<typeof import("../../bridge/tauri/invoke")>()),
  invoke: (...a: unknown[]) => invokeMock(...a),
}));

const COVERAGE = "Checked for email addresses and more.";
const clean = (t: string): DisclosureScan => ({ scanner: "citrate-pii-scan/1", passed: true, found: [], redacted: t, coverage: COVERAGE });

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const ME = "0x1111111111111111111111111111111111111111";
const DANA = "0x2222222222222222222222222222222222222222";
const CARL = "0x3333333333333333333333333333333333333333";
const ROSTER: GroupMember[] = [
  { address: DANA, role: "owner" },
  { address: ME, role: "admin" },
];
const GROUP: Group = { id: "g-char", name: "Orchard Lab", owner: DANA, kind: "forum", members: ROSTER };
const TODAY: RoundsToday = { kind: "not-live", detail: "Both ledgers are deployed, no rounds." };
const HISTORY_KEY = "citrate.groups.history.v1";

let host: HTMLDivElement;
let root: Root;

const store = (): Store =>
  ({
    toast: vi.fn(),
    go: vi.fn(),
    identity: () => ({ wallet: "", name: "Me" }),
    refreshPeople: async () => {},
    requestSig: vi.fn(),
    canJoinNow: () => true,
    openInviteLink: () => {},
    clearInviteLink: () => {},
  }) as unknown as Store;
const state = (over: Partial<AppState> = {}): AppState => ({ ...freshState("p1"), commsAddr: ME, ...over });

const msg = (id: string, sender: string, body: string, ts: number): GroupMessage => ({ id, groupId: GROUP.id, sender, body, ts });

function seed(messages: GroupMessage[] = [], roster: GroupMember[] = ROSTER) {
  vi.spyOn(bridge.groups, "list").mockResolvedValue([GROUP]);
  vi.spyOn(bridge.groups, "roster").mockResolvedValue(roster);
  vi.spyOn(bridge.groups, "messages").mockResolvedValue([]);
  groupsSlice.set({ groups: [GROUP], selectedId: GROUP.id, roster, messages, history: { [GROUP.id]: messages }, names: {}, error: null, sending: false });
}

beforeEach(() => {
  vi.restoreAllMocks();
  localStorage.clear();
  invokeMock.mockReset();
  invokeMock.mockRejectedValue(new Error("no Tauri runtime in this test"));
  scanMock.fn = async (t) => clean(t);
  vi.spyOn(bridge.groups, "selfAddress").mockResolvedValue(ME);
  vi.spyOn(bridge.social, "resolve").mockResolvedValue([]);
  vi.spyOn(bridge.social, "ingestBinding").mockResolvedValue(false);
  vi.spyOn(bridge.cluster, "deviceLinksIngest").mockResolvedValue({ links: 0, revocations: 0 } as never);
  vi.spyOn(bridge.cluster, "devices").mockRejectedValue(new Error("cluster daemon not running"));
  vi.spyOn(bridge.cluster, "status").mockRejectedValue(new Error("cluster daemon not running"));
  vi.spyOn(bridge.invites, "myRequests").mockResolvedValue([]);
  vi.spyOn(bridge.invites, "requests").mockResolvedValue([]);
  vi.spyOn(bridge.contracts, "codeSize").mockResolvedValue(100);
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  vi.useRealTimers();
  groupsSlice.set({ groups: [], selectedId: null, roster: [], messages: [], history: {}, names: {}, error: null });
});

async function render(over: Partial<AppState> = {}) {
  await act(async () => root.render(<GroupChat store={store()} s={state(over)} view="groups" rounds={TODAY} />));
}
const btn = (label: string) => [...host.querySelectorAll("button")].find((b) => b.textContent === label) as HTMLButtonElement | undefined;
const setValue = async (el: HTMLTextAreaElement | HTMLInputElement, v: string) => {
  const proto = el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
  Object.getOwnPropertyDescriptor(proto, "value")!.set!.call(el, v);
  await act(async () => el.dispatchEvent(new Event("input", { bubbles: true })));
};
/** The words of each plain message on screen, top to bottom. */
const shownTexts = () =>
  [...host.querySelectorAll("[data-testid=group-message] .grp-msg-body")].map((b) => {
    const c = b.cloneNode(true) as HTMLElement;
    c.querySelectorAll(".grp-msg-meta, .grp-inline, .grp-replies").forEach((e) => e.remove());
    return (c.textContent ?? "").trim();
  });
/** The sender name of each plain message on screen, top to bottom. */
const shownNames = () => [...host.querySelectorAll("[data-testid=group-message] .grp-msg-meta b")].map((b) => b.textContent);

describe("L4 characterization: the group thread", () => {
  it("char_thread_renders_oldest_first", async () => {
    seed([msg("1", DANA, "first", 1), msg("2", DANA, "second", 2), msg("3", DANA, "third", 3)]);
    await render();
    const thread = host.querySelector("[data-testid=group-thread]") as HTMLElement;
    const t = thread.textContent ?? "";
    expect(t.indexOf("first")).toBeLessThan(t.indexOf("second"));
    expect(t.indexOf("second")).toBeLessThan(t.indexOf("third"));
    expect(shownTexts()).toEqual(["first", "second", "third"]);
    // The retained history is kept oldest first by receipt time, whatever order a drain returns.
    vi.spyOn(bridge.groups, "messages").mockResolvedValue([msg("5", DANA, "fifth", 5), msg("4", DANA, "fourth", 4)]);
    await act(async () => reloadMessages(GROUP.id));
    expect(groupsSlice.get().history[GROUP.id].map((m) => m.body)).toEqual(["first", "second", "third", "fourth", "fifth"]);
    expect(shownTexts()).toEqual(["first", "second", "third", "fourth", "fifth"]);
  });

  it("char_composer_below_thread", async () => {
    seed([msg("1", DANA, "hello", 1)]);
    await render();
    const thread = host.querySelector("[data-testid=group-thread]") as HTMLElement;
    const composer = host.querySelector(".grp-composer") as HTMLElement;
    expect(thread).not.toBeNull();
    expect(composer).not.toBeNull();
    expect(composer.contains(host.querySelector("#grp-msg"))).toBe(true);
    expect(thread.compareDocumentPosition(composer) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    // The composer is a sibling after the scrolling thread, not inside it.
    expect(thread.contains(composer)).toBe(false);
    expect(thread.parentElement).toBe(composer.parentElement);
  });

  it("char_replies_oldest_first_in_fold", async () => {
    const parent = "Could we run a round this week?";
    const to = messageRef(DANA, parent);
    const messages = [
      msg("1", DANA, parent, 1),
      msg("2", ME, encodeReply(to, "reply one"), 2),
      msg("3", DANA, encodeReply(to, "reply two"), 3),
    ];
    // The model: replies fold under their parent in the order they arrived (push order).
    const model = threadModel(messages);
    expect(model).toHaveLength(1);
    expect(model[0].replies.map((r) => r.text)).toEqual(["reply one", "reply two"]);
    // The fold is closed until opened, then lists the replies oldest first.
    seed(messages);
    await render();
    const toggle = btn("2 replies")!;
    expect(toggle.getAttribute("aria-expanded")).toBe("false");
    expect(host.querySelector("[aria-label=Replies]")).toBeNull();
    await act(async () => toggle.click());
    const rows = [...host.querySelectorAll("[aria-label=Replies] [role=listitem]")].map((r) => r.textContent);
    expect(rows).toEqual(["You reply one", "0x2222…2222 reply two"]);
  });

  it("char_name_is_handle_else_short_address", async () => {
    const resolve = vi.spyOn(bridge.social, "resolve").mockResolvedValue([{ address: DANA, network: "discord", handle: "dana" }]);
    seed([msg("1", DANA, "from dana", 1), msg("2", CARL, "from carl", 2), msg("3", ME, "from me", 3)]);
    await render();
    // A verified face shows its handle; any other address its short form; this device is "You".
    expect(shownNames()).toEqual(["@dana", "0x3333…3333", "You"]);
    // Faces are looked up by the addresses as the comms daemon reports them (roster then senders).
    expect(resolve.mock.calls.at(-1)?.[0]).toEqual([DANA, ME, CARL]);
  });

  it("char_short_address_format", async () => {
    seed([msg("1", DANA, "long", 1), msg("2", "0xabcdef", "short", 2), msg("3", "0x0123456789", "twelve", 3)]);
    await render();
    // More than 12 characters: the first 6, an ellipsis, the last 4. Up to 12: as it is.
    expect(shownNames()).toEqual(["0x2222…2222", "0xabcdef", "0x0123456789"]);
  });

  it("char_poll_every_4s", async () => {
    seed();
    vi.useFakeTimers({ toFake: ["setInterval", "clearInterval"] });
    const drain = vi.spyOn(bridge.groups, "messages").mockResolvedValue([]);
    await render();
    const c0 = drain.mock.calls.length;
    await act(async () => vi.advanceTimersByTime(3999));
    expect(drain.mock.calls.length).toBe(c0);
    await act(async () => vi.advanceTimersByTime(1));
    expect(drain.mock.calls.length).toBe(c0 + 1);
    expect(drain.mock.calls.at(-1)?.[0]).toBe(GROUP.id);
    await act(async () => vi.advanceTimersByTime(4000));
    expect(drain.mock.calls.length).toBe(c0 + 2);
    // The poll stops when the board goes away.
    await act(async () => root.unmount());
    await act(async () => vi.advanceTimersByTime(8000));
    expect(drain.mock.calls.length).toBe(c0 + 2);
  });
});

describe("L4 characterization: retained history", () => {
  it("char_history_persisted_to_localstorage_key_v1", async () => {
    seed();
    vi.spyOn(bridge.groups, "messages").mockResolvedValue([msg("rx1", DANA, "kept on this device", 10)]);
    await reloadMessages(GROUP.id);
    const raw = localStorage.getItem(HISTORY_KEY);
    expect(raw).not.toBeNull();
    const saved = JSON.parse(raw!) as Record<string, GroupMessage[]>;
    expect(Object.keys(saved)).toEqual([GROUP.id]);
    expect(saved[GROUP.id]).toEqual([msg("rx1", DANA, "kept on this device", 10)]);
    // Plain JSON under one key for every group; no other key is written.
    expect(Array.from({ length: localStorage.length }, (_, i) => localStorage.key(i))).toEqual([HISTORY_KEY]);
  });

  it("char_history_cap_500", async () => {
    seed();
    const drained = Array.from({ length: 501 }, (_, i) => msg(`rx${i}`, DANA, `m${i}`, i + 1));
    vi.spyOn(bridge.groups, "messages").mockResolvedValue(drained);
    await reloadMessages(GROUP.id);
    const kept = groupsSlice.get().history[GROUP.id];
    expect(kept).toHaveLength(500);
    // The oldest message is the one dropped.
    expect(kept[0].body).toBe("m1");
    expect(kept[499].body).toBe("m500");
    expect((JSON.parse(localStorage.getItem(HISTORY_KEY)!) as Record<string, GroupMessage[]>)[GROUP.id]).toHaveLength(500);
  });

  it("char_ts_is_receipt_time", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(1_700_000_000_000);
    invokeMock.mockResolvedValue([
      [DANA, "one"],
      [CARL, "two"],
    ]);
    const rows = await tauriGroups.messages(GROUP.id);
    expect(invokeMock).toHaveBeenCalledWith("groups_messages", { group: GROUP.id });
    // The daemon gives (sender, body) only: every message in a drain gets this device's receipt
    // time, never a send time.
    expect(rows.map((r) => r.ts)).toEqual([1_700_000_000_000, 1_700_000_000_000]);
    expect(rows.map((r) => [r.sender, r.body, r.groupId])).toEqual([
      [DANA, "one", GROUP.id],
      [CARL, "two", GROUP.id],
    ]);
    vi.setSystemTime(1_700_000_005_000);
    const later = await tauriGroups.messages(GROUP.id);
    expect(later[0].ts).toBe(1_700_000_005_000);
  });
});

describe("L4 characterization: what is never shown, and Hermes", () => {
  it("char_control_messages_hidden", async () => {
    const bindingBody = SOCIAL_BINDING_MSG_PREFIX + JSON.stringify({ network: "discord", handle: "dana", address: DANA, nonce: "n", signature: "0x00" });
    const linkBody = DEVICE_LINKS_MSG_PREFIX + "{}";
    const marked = "\u0001(a structure line from a newer app)";
    // Both shipped prefixes start with U+0001 (domains.ts holds the byte itself, not an escape).
    expect(SOCIAL_BINDING_MSG_PREFIX).toBe("\u0001cbind1:");
    expect(DEVICE_LINKS_MSG_PREFIX).toBe("\u0001cdlink1:");
    const messages = [msg("1", DANA, bindingBody, 1), msg("2", DANA, linkBody, 2), msg("3", DANA, marked, 3), msg("4", DANA, "hello", 4)];
    expect(threadModel(messages).map((t) => t.text)).toEqual(["hello"]);
    expect(lastLine(messages.slice(0, 3))).toBe("");
    // Nor do they reach the on-device model: the Hermes transcript is built from the same thread.
    const req = buildRequest({ groupName: GROUP.name, roster: ROSTER, cluster: null, rounds: "not live", messages, asker: ME });
    expect(req.messagesJson).toContain("hello");
    for (const hidden of ["cbind1", "cdlink1", "newer app", "\\u0001"]) expect(req.messagesJson).not.toContain(hidden);
    seed(messages);
    await render();
    expect(shownTexts()).toEqual(["hello"]);
    const thread = host.querySelector("[data-testid=group-thread]")?.textContent ?? "";
    expect(thread).not.toContain("cbind1");
    expect(thread).not.toContain("cdlink1");
    expect(thread).not.toContain("newer app");
    // The binding is handed to the social store for checking, not shown.
    expect(bridge.social.ingestBinding).toHaveBeenCalledWith(DANA, expect.objectContaining({ handle: "dana", address: DANA }));
  });

  it("char_hermes_disclosure_line_present", async () => {
    expect(HERMES_DISCLOSURE).toBe("@Hermes runs on your device. You see its answer, checked for private information, before you post it to everyone in this group.");
    seed();
    await render();
    const line = [...host.querySelectorAll(".grp-composer .grp-faint")].map((e) => e.textContent);
    expect(line).toEqual([HERMES_DISCLOSURE]);
  });

  it("char_send_card_before_post", async () => {
    seed();
    vi.spyOn(bridge.chat, "inferLocal").mockResolvedValue("Five machines reported hardware.");
    const send = vi.spyOn(bridge.groups, "send").mockResolvedValue();
    await render();
    const ask = "@Hermes what would that take?";
    await setValue(host.querySelector("#grp-msg") as HTMLTextAreaElement, ask);
    await act(async () => btn("Send")!.click());
    // The member's message is sent; Hermes's answer waits on the card and is not posted yet.
    expect(send).toHaveBeenCalledTimes(1);
    expect(send.mock.calls[0]).toEqual([GROUP.id, ask]);
    const card = host.querySelector("[data-testid=group-send-card]") as HTMLElement;
    expect(card.getAttribute("role")).toBe("dialog");
    expect(card.querySelector("[data-testid=group-send-preview]")?.textContent).toBe("Five machines reported hardware.");
    await act(async () => btn("Post to 1 member")!.click());
    expect(send).toHaveBeenCalledTimes(2);
    const posted = decodeHermes(send.mock.calls[1][1] as string);
    expect(posted?.to).toBe(messageRef(ME, ask));
    expect(posted?.text).toBe("Five machines reported hardware.");
    expect(host.querySelector("[data-testid=group-send-card]")).toBeNull();
  });

  it("char_role_grant_request_shape", async () => {
    invokeMock.mockResolvedValue(undefined);
    await tauriGroups.assignRole(GROUP.id, DANA, "admin");
    // The UI's lowercase role goes to `groups_assign_role` as is; comms.rs maps it to the daemon's
    // `grantRole` (pinned in src-tauri/src/comms_tests.rs).
    expect(invokeMock).toHaveBeenCalledTimes(1);
    expect(invokeMock).toHaveBeenCalledWith("groups_assign_role", { group: GROUP.id, member: DANA, role: "admin" });
  });
});
