// D-12 — the invitee's side of the one invite flow: a deep link and a pasted link open the SAME
// "Join <group>?" screen; Join sends a request (never a join); the request's states read honestly.
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { store, useStore } from "../shell/store";
import { bridge } from "../bridge";
import type { InvitePreview, MyInviteRequest } from "../bridge/domains";
import { HaveAnInvite } from "./HaveAnInvite";
import { isInviteLink } from "./inviteLink";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const LINK = "citrate://invite?g=abababababababababababababababab&t=tok&k=02ab&e=2000000000&n=47617264656e20636c7562&by=4c61727279";
const preview: InvitePreview = {
  group: "abababababababababababababababab",
  groupName: "Garden club",
  inviter: "Larry",
  expiresAt: 2_000_000_000,
  expired: false,
  requested: false,
};
const req = (state: MyInviteRequest["state"], note = ""): MyInviteRequest => ({
  id: "0123456789abcdef",
  group: preview.group,
  groupName: "Garden club",
  inviter: "Larry",
  requestedAt: 1_900_000_000,
  expiresAt: 2_000_000_000,
  state,
  note,
});

function Live({ memberOf = [] as string[] }) {
  const s = useStore();
  return <HaveAnInvite store={store} s={s} memberOf={memberOf} />;
}

let host: HTMLDivElement;
let root: Root;
async function mount(el: React.ReactElement) {
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
  await act(async () => {
    root.render(el);
  });
  await act(async () => {});
}
const q = (id: string) => host.querySelector(`[data-testid="${id}"]`) as HTMLElement | null;
async function click(el: Element | null) {
  expect(el).toBeTruthy();
  await act(async () => {
    (el as HTMLElement).click();
  });
  await act(async () => {});
}
async function type(el: HTMLInputElement | null, value: string) {
  expect(el).toBeTruthy();
  await act(async () => {
    const set = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")?.set;
    set?.call(el, value);
    el!.dispatchEvent(new Event("input", { bubbles: true }));
  });
}

beforeEach(() => {
  store.setState({ inviteLink: null, stage: "done", signedIn: true, route: "home" });
  vi.spyOn(bridge.invites, "myRequests").mockResolvedValue([]);
  vi.spyOn(bridge.invites, "preview").mockResolvedValue(preview);
});
afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  vi.restoreAllMocks();
});

describe("a deep link and a pasted link open the same Join screen", () => {
  it("a citrate://invite deep link opens Join <group>? on Groups", async () => {
    store.handleDeepLink(LINK);
    expect(store.state.inviteLink).toBe(LINK);
    expect(store.state.route).toMatch(/^groups/);
    await mount(<Live />);
    expect(q("invite-join")?.getAttribute("data-phase")).toBe("ready");
    // The group and sender are shown as what the link says, never as checked facts.
    expect(q("invite-join")?.textContent).toContain("This link invites you to Garden club.");
    expect(q("invite-join")?.textContent).toContain("The link says it's from Larry.");
    expect(q("invite-join")?.textContent).not.toContain("Larry invited you");
    expect(bridge.invites.preview).toHaveBeenCalledWith(LINK);
  });

  it("pasting the link opens the same screen", async () => {
    await mount(<Live />);
    expect(q("invite-join")).toBeNull();
    await click(q("invite-paste-open"));
    await type(q("invite-paste-input") as HTMLInputElement, `  ${LINK}  `);
    await click(q("invite-paste-submit"));
    expect(store.state.inviteLink).toBe(LINK);
    expect(q("invite-join")?.textContent).toContain("This link invites you to Garden club.");
  });

  it("anything else pasted is refused and opens nothing", async () => {
    await mount(<Live />);
    await click(q("invite-paste-open"));
    await type(q("invite-paste-input") as HTMLInputElement, "https://citrate.ai/join/Ab3dE7gh#key");
    await click(q("invite-paste-submit"));
    expect(store.state.inviteLink).toBeNull();
    expect(host.textContent).toContain("isn't an invite link");
  });

  it("Groups and onboarding render the same component, and no other paste field exists", () => {
    const src = (p: string) => readFileSync(resolve(__dirname, p), "utf8");
    expect(src("./Groups.tsx")).toContain("<HaveAnInvite ");
    expect(src("../onboarding/Onboarding.tsx")).toContain("<HaveAnInvite ");
    for (const f of ["./Groups.tsx", "../onboarding/Onboarding.tsx"]) {
      expect(src(f)).not.toMatch(/InvitePasteField|acceptInviteInput|invites\.request\(/);
    }
  });
});

describe("Join sends a request, never a join", () => {
  it("Join sends the request, closes the screen and shows Waiting for <inviter>", async () => {
    const request = vi.spyOn(bridge.invites, "request").mockResolvedValue(req("waiting"));
    store.openInviteLink(LINK);
    await mount(<Live />);
    vi.mocked(bridge.invites.myRequests).mockResolvedValue([req("waiting")]);
    await click(q("invite-join-send"));
    expect(request).toHaveBeenCalledWith(LINK);
    expect(store.state.inviteLink).toBeNull();
    expect(q("invite-request")?.getAttribute("data-state")).toBe("waiting");
    expect(q("invite-request")?.textContent).toContain("Waiting for your request to join Garden club to be approved");
    expect(host.textContent).not.toMatch(/you're in/i);
  });

  it("a failed request says why and offers Try again", async () => {
    vi.spyOn(bridge.invites, "request").mockRejectedValue(new Error("Couldn't send your request: relay unreachable"));
    store.openInviteLink(LINK);
    await mount(<Live />);
    await click(q("invite-join-send"));
    expect(host.textContent).toContain("relay unreachable");
    expect(q("invite-join-send")?.textContent).toBe("Try again");
    expect(store.state.inviteLink).toBe(LINK);
  });

  it("an expired link says so and has no Join", async () => {
    vi.mocked(bridge.invites.preview).mockResolvedValue({ ...preview, expired: true });
    const request = vi.spyOn(bridge.invites, "request");
    store.openInviteLink(LINK);
    await mount(<Live />);
    expect(q("invite-join")?.getAttribute("data-phase")).toBe("expired");
    expect(q("invite-join-send")).toBeNull();
    expect(request).not.toHaveBeenCalled();
  });

  it("a link from an earlier version shows Rust's reason", async () => {
    vi.mocked(bridge.invites.preview).mockRejectedValue(new Error("This invite link is from an earlier version of Citrate and can't be used. Ask for a new link."));
    store.openInviteLink("citrate://invite?g=g&t=t&k=k");
    await mount(<Live />);
    expect(q("invite-join")?.getAttribute("data-phase")).toBe("error");
    expect(host.textContent).toContain("earlier version");
  });

  it("already a member: says so instead of offering Join", async () => {
    store.openInviteLink(LINK);
    await mount(<Live memberOf={[preview.group]} />);
    expect(q("invite-join")?.getAttribute("data-phase")).toBe("member");
    expect(q("invite-join-send")).toBeNull();
  });

  it("before the wallet step Join is held, and the invite stays", async () => {
    store.setState({ stage: "s1", signedIn: false });
    store.openInviteLink(LINK);
    await mount(<Live />);
    expect((q("invite-join-send") as HTMLButtonElement).disabled).toBe(true);
    expect(host.textContent).toContain("once setup reaches the wallet step");
  });
});

describe("the requests this device sent", () => {
  it.each([
    ["declined", "Your request to join Garden club was declined."],
    ["revoked", "This invite to Garden club was cancelled."],
    ["expired", "Your invite to Garden club expired before it was approved."],
  ] as const)("%s reads honestly", async (state, text) => {
    vi.mocked(bridge.invites.myRequests).mockResolvedValue([req(state)]);
    await mount(<Live />);
    expect(q("invite-request")?.getAttribute("data-state")).toBe(state);
    expect(q("invite-request")?.textContent).toContain(text);
  });

  it("an unreachable relay is a note on a waiting request, never an answer", async () => {
    vi.mocked(bridge.invites.myRequests).mockResolvedValue([req("waiting", "Can't reach the relay right now (offline).")]);
    await mount(<Live />);
    expect(q("invite-request")?.getAttribute("data-state")).toBe("waiting");
    expect(q("invite-request-note")?.textContent).toContain("Can't reach the relay");
  });

  it("approved: You're in <the joined group's own name>, until dismissed", async () => {
    vi.mocked(bridge.invites.myRequests).mockResolvedValue([req("joined")]);
    // The link said "Garden club"; the joined group's own state names it "Rose growers".
    vi.spyOn(bridge.groups, "list").mockResolvedValue([{ id: preview.group, name: "Rose growers", kind: "channel", owner: "0x1", members: [] } as never]);
    await mount(<Live />);
    expect(q("invite-request")?.textContent).toContain("You're in Rose growers.");
    expect(q("invite-request")?.textContent).not.toContain("Garden club");
    await click(q("invite-request-dismiss"));
    expect(q("invite-request")).toBeNull();
  });

  it("Cancel request forgets it in Rust", async () => {
    vi.mocked(bridge.invites.myRequests).mockResolvedValue([req("waiting")]);
    const dismiss = vi.spyOn(bridge.invites, "dismiss").mockResolvedValue();
    await mount(<Live />);
    await click(q("invite-request-dismiss"));
    expect(dismiss).toHaveBeenCalledWith("0123456789abcdef");
    expect(q("invite-request")).toBeNull();
  });
});

describe("isInviteLink", () => {
  it("knows only citrate://invite links", () => {
    expect(isInviteLink(LINK)).toBe(true);
    expect(isInviteLink(` ${LINK} `)).toBe(true);
    for (const s of ["", "citrate://join/Ab3dE7gh#k", "https://citrate.ai/join/Ab3dE7gh", "citrate://pair?c=x", "citrate://invite?" + "x".repeat(2100)]) {
      expect(isInviteLink(s)).toBe(false);
    }
  });
});
