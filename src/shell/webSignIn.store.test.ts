// HUP-S2.3: a sign-in request from Hermes's managed browser in the store. The main window names
// the request and nothing else; a card opens the ordinary review gate, approval goes to core's
// sign-in command (never to signing.broadcast) and a decline tells the page.
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { store } from "./store";
import { bridge } from "../bridge";
import { signInApi, type SignInApi } from "../budgets/signIn";
import type { CeremonyView } from "../bridge/types";

const view: CeremonyView = {
  id: "c-9",
  origin: "https://app.example.org",
  kind: "personal_sign",
  chainId: 40204,
  decoded: { action: 'Sign message: "app.example.org wants you to sign in…"', cost: "no funds moved", destination: "https://app.example.org" },
  requiresRawAck: false,
};

describe("HUP-S2.3 web sign-in in the store", () => {
  let api: { [K in keyof SignInApi]: ReturnType<typeof vi.fn> };
  let saved: SignInApi | null;
  let broadcastSpy: ReturnType<typeof vi.spyOn>;
  let signingRejectSpy: ReturnType<typeof vi.spyOn>;

  beforeEach(() => {
    saved = signInApi.current;
    api = {
      request: vi.fn(async () => ({ outcome: "pending", ceremony: view, reason: "this site has no sign-in budget" })),
      approve: vi.fn(async () => true),
      reject: vi.fn(async () => undefined),
    };
    signInApi.current = api as unknown as SignInApi;
    store.setState({ walletReview: null, toast: null } as never);
    broadcastSpy = vi.spyOn(bridge.signing, "broadcast").mockResolvedValue({ txHash: "0xhash", blockNumber: 1 });
    signingRejectSpy = vi.spyOn(bridge.signing, "reject").mockResolvedValue(undefined);
  });
  afterEach(() => {
    vi.restoreAllMocks();
    signInApi.current = saved;
    store.setState({ walletReview: null });
  });

  it("a card opens the review with core's reason and signs nothing", async () => {
    await store.handleWebSignIn("signin-1-1");
    expect(api.request).toHaveBeenCalledWith("signin-1-1");
    expect(store.state.walletReview?.kind).toBe("web-sign-in");
    expect(store.state.walletReview?.label).toBe("Sign in to https://app.example.org");
    expect(store.state.walletReview?.hic?.reason).toBe("this site has no sign-in budget");
    expect(api.approve).not.toHaveBeenCalled();
  });

  it("approve goes to core's sign-in command, never to broadcast", async () => {
    await store.handleWebSignIn("signin-1-1");
    await store.approveWalletReview();
    expect(api.approve).toHaveBeenCalledWith("c-9", false);
    expect(broadcastSpy).not.toHaveBeenCalled();
    expect(store.state.walletReview).toBeNull();
    expect(store.state.toast).toContain("Signed in");
  });

  it("a page that left before the signature is reported honestly", async () => {
    api.approve.mockResolvedValueOnce(false);
    await store.handleWebSignIn("signin-1-1");
    await store.approveWalletReview();
    expect(store.state.toast).toContain("did not receive the signature");
  });

  it("decline uses the sign-in reject, not the generic one", async () => {
    await store.handleWebSignIn("signin-1-1");
    await store.rejectWalletReview();
    expect(api.reject).toHaveBeenCalledWith("c-9");
    expect(signingRejectSpy).not.toHaveBeenCalled();
    expect(store.state.walletReview).toBeNull();
  });

  it("a request arriving while another review is open is declined, not queued", async () => {
    store.setState({ walletReview: { kind: "send", label: "Send", view: { ...view, id: "other" }, rawAck: false } } as never);
    await store.handleWebSignIn("signin-1-1");
    expect(api.reject).toHaveBeenCalledWith("c-9");
    expect(store.state.walletReview?.view.id).toBe("other");
  });

  it("an automatic sign-in opens nothing (the notice announces it); a refusal is explained", async () => {
    api.request.mockResolvedValueOnce({ outcome: "auto_signed", origin: "https://app.example.org", remaining: 2, recordId: 3, budgetId: 1, delivered: true });
    await store.handleWebSignIn("signin-1-1");
    expect(store.state.walletReview).toBeNull();
    api.request.mockResolvedValueOnce({ outcome: "refused", reason: "Citrate shares your address only with a site you gave a sign-in budget" });
    await store.handleWebSignIn("signin-1-2");
    expect(store.state.toast).toContain("sign-in budget");
  });

  it("does nothing outside the desktop app", async () => {
    signInApi.current = null;
    await store.handleWebSignIn("signin-1-1");
    expect(store.state.walletReview).toBeNull();
  });
});
