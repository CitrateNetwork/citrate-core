// D-12 — the inviter's side: one [Invite someone] button and one Pending list (open links with
// [Revoke], requests with [Approve] / [Decline]). Unknown is never shown as "nothing pending".
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { store } from "../shell/store";
import { bridge } from "../bridge";
import type { IncomingRequest, InviteView } from "../bridge/domains";
import { InvitePanel, checkedToast, declinedToast, revokedToast } from "./InvitePanel";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const G = "abababababababababababababababab";
const ANA = "0x00000000000000000000000000000000000000aa";
const inv: InviteView = { id: "0123456789abcdef", group: G, link: "citrate://invite?g=x&t=y&k=z&e=1", label: "", createdAt: 1, expiresAt: 2_000_000_000, approving: "" };
const ask: IncomingRequest = { inviteId: inv.id, group: G, address: ANA, inviteLabel: "", approving: false };

let host: HTMLDivElement;
let root: Root;
async function mount(onAdded = () => {}) {
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
  await act(async () => {
    root.render(<InvitePanel store={store} groupId={G} inviterName="Larry" onAdded={onAdded} />);
  });
  await act(async () => {});
}
const q = (id: string) => host.querySelector(`[data-testid="${id}"]`) as HTMLElement | null;
const all = (id: string) => Array.from(host.querySelectorAll(`[data-testid="${id}"]`));
async function click(el: Element | null) {
  expect(el).toBeTruthy();
  await act(async () => {
    (el as HTMLElement).click();
  });
  await act(async () => {});
}

let toast: ReturnType<typeof vi.spyOn>;
beforeEach(() => {
  toast = vi.spyOn(store, "toast").mockImplementation(() => {});
  vi.spyOn(bridge.invites, "list").mockResolvedValue([]);
  vi.spyOn(bridge.invites, "requests").mockResolvedValue([]);
});
afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  vi.restoreAllMocks();
});

describe("InvitePanel", () => {
  it("Invite someone makes a link, copies it and lists it under Pending", async () => {
    const writeText = vi.fn(async () => {});
    Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
    const create = vi.spyOn(bridge.invites, "create").mockResolvedValue(inv);
    await mount();
    vi.mocked(bridge.invites.list).mockResolvedValue([inv]);
    await click(q("invite-someone"));
    expect(create).toHaveBeenCalledWith(G, "", "Larry");
    expect(writeText).toHaveBeenCalledWith(inv.link);
    expect(all("invite-open")).toHaveLength(1);
    expect(String(toast.mock.calls[0][0])).toContain("approve");
    expect(String(toast.mock.calls[0][0])).not.toMatch(/they're in|no approval/i);
  });

  it("a request shows who, with Approve and Decline; Approve adds them", async () => {
    vi.mocked(bridge.invites.list).mockResolvedValue([inv]);
    vi.mocked(bridge.invites.requests).mockResolvedValue([ask]);
    const approve = vi.spyOn(bridge.invites, "approve").mockResolvedValue();
    const onAdded = vi.fn();
    await mount(onAdded);
    // What the request says, not a verified identity: the owner confirms it's them.
    expect(q("invite-request-in")?.textContent).toContain("A request to join from 0x0000…00aa");
    expect(q("invite-request-in")?.textContent).toContain("confirm it's them before approving");
    expect(q("invite-request-in")?.textContent).not.toContain("wants to join");
    await click(q("invite-approve"));
    expect(approve).toHaveBeenCalledWith(G, inv.id, ANA);
    expect(onAdded).toHaveBeenCalled();
  });

  it("a refused approval (revoked or used) is shown, and nobody is reported added", async () => {
    vi.mocked(bridge.invites.list).mockResolvedValue([inv]);
    vi.mocked(bridge.invites.requests).mockResolvedValue([ask]);
    vi.spyOn(bridge.invites, "approve").mockRejectedValue(new Error("This invite was revoked or already used, so this request can't be approved."));
    const onAdded = vi.fn();
    await mount(onAdded);
    await click(q("invite-approve"));
    expect(onAdded).not.toHaveBeenCalled();
    expect(String(toast.mock.calls.at(-1)?.[0])).toContain("revoked or already used");
  });

  it("Decline and Revoke say whether the requester could be told", async () => {
    vi.mocked(bridge.invites.list).mockResolvedValue([inv]);
    vi.mocked(bridge.invites.requests).mockResolvedValue([ask]);
    vi.spyOn(bridge.invites, "decline").mockResolvedValue({ notified: false });
    vi.spyOn(bridge.invites, "revoke").mockResolvedValue({ notified: true });
    await mount();
    await click(q("invite-decline"));
    expect(toast).toHaveBeenLastCalledWith(declinedToast(false));
    await click(q("invite-revoke"));
    expect(bridge.invites.revoke).toHaveBeenCalledWith(G, inv.id);
    expect(toast).toHaveBeenLastCalledWith(revokedToast(true));
    expect(declinedToast(false)).toMatch(/couldn't be told/);
  });

  it("saved invites that can't be read are an error, not an empty Pending list", async () => {
    vi.mocked(bridge.invites.list).mockRejectedValue(new Error("Your saved invites are damaged and can't be read."));
    await mount();
    expect(q("invite-list-error")?.textContent).toContain("damaged");
  });

  it("a relay that can't be asked is said, not shown as no requests", async () => {
    vi.mocked(bridge.invites.list).mockResolvedValue([inv]);
    vi.mocked(bridge.invites.requests).mockRejectedValue(new Error("Couldn't check for requests: relay unreachable"));
    await mount();
    expect(q("invite-requests-error")?.textContent).toContain("relay unreachable");
  });

  it("a held approval is settled from the invite row: Check again and Cancel, even with no request rows", async () => {
    // The relay restarted: no request rows. The invite row alone offers the way out.
    vi.mocked(bridge.invites.list).mockResolvedValue([{ ...inv, approving: ANA }]);
    vi.mocked(bridge.invites.requests).mockResolvedValue([]);
    const check = vi.spyOn(bridge.invites, "check").mockResolvedValue("released");
    const revoke = vi.spyOn(bridge.invites, "revoke").mockResolvedValue({ notified: true });
    await mount();
    expect(q("invite-open")?.textContent).toContain("not confirmed yet");
    expect(q("invite-revoke")).toBeNull();
    await click(q("invite-check"));
    expect(check).toHaveBeenCalledWith(G, inv.id);
    expect(toast).toHaveBeenLastCalledWith(checkedToast("released"));
    await click(q("invite-cancel"));
    expect(revoke).toHaveBeenCalledWith(G, inv.id);
  });

  it("a held request row offers neither Approve nor Decline (the invite row settles it)", async () => {
    vi.mocked(bridge.invites.list).mockResolvedValue([{ ...inv, approving: ANA }]);
    vi.mocked(bridge.invites.requests).mockResolvedValue([{ ...ask, approving: true }]);
    await mount();
    expect(q("invite-approve")).toBeNull();
    expect(q("invite-decline")).toBeNull();
    expect(q("invite-check")).toBeTruthy();
  });

  it("the owner can label a link, and each request says which link it came in on", async () => {
    const create = vi.spyOn(bridge.invites, "create").mockResolvedValue({ ...inv, label: "for Ana" });
    vi.mocked(bridge.invites.list).mockResolvedValue([{ ...inv, label: "for Ana" }]);
    vi.mocked(bridge.invites.requests).mockResolvedValue([{ ...ask, inviteLabel: "for Ana" }]);
    await mount();
    expect(q("invite-request-in")?.textContent).toContain("through “for Ana”");
    const input = q("invite-label") as HTMLInputElement;
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")?.set?.call(input, "  for Bo ");
      input.dispatchEvent(new Event("input", { bubbles: true }));
    });
    await click(q("invite-someone"));
    expect(create).toHaveBeenCalledWith(G, "for Bo", "Larry");
  });

});
