// K6-3 (K6e): a signature the network gate refuses shows the gate's own reason, as is. The gate
// already says nothing was signed; the app adds only "not sent" and never rewords, shortens or
// replaces the reason. #494: a network check that could not read the endpoint before the send is
// "not sent" too (the backend marks it "not submitted: "), never "outcome unknown".
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

vi.mock("../bridge/mode", () => ({
  BRIDGE_MODE: "tauri",
  assertSimAllowed: () => {},
}));

import { store } from "./store";
import { bridge } from "../bridge";
import { createCeremonyProvider } from "../wallet/ceremonyConnector";
import type { CeremonyView } from "../bridge/types";

const view: CeremonyView = {
  id: "wcer-net",
  origin: "local-user",
  kind: "transaction",
  chainId: 1,
  decoded: { action: "Send 1.00 SALT", cost: "est. gas 0.0019 SALT", destination: "0x" + "ab".repeat(20) },
  requiresRawAck: false,
};

// The exact strings the Rust gate produces (kit rpc.rs check_one / verify_network, carried by
// CeremonyError::WrongNetwork "ceremony: {m}; nothing was signed").
const NOT_SET =
  'ceremony: this build has no network set (profile "genesis-2026-12"), so the wallet signs nothing; update Citrate Core; nothing was signed';
const RETIRED =
  "ceremony: broadcast failed: network check refused: the endpoint's block 0 is 0x98e0d72f422049606a6b29ca0a9bcfd2300753fd36526d2c8e9f9a2531b70c73, this build targets 0x1dcfc4909dd9fdcb0f03c823d627e4b2032cc86bef5fc4561030d6c64b9775f1 (alpha-r1005); update Citrate Core";
const UNREADABLE =
  "ceremony: broadcast failed: the network could not be checked before sending, so nothing was sent: rpc transport error: connection refused";

const lastToast = () => String(store.getSnapshot().toast ?? "");

describe("a refused signature shows the gate's reason as is (K6-3)", () => {
  let onResolved: ReturnType<typeof vi.fn>;
  beforeEach(() => {
    vi.spyOn(bridge.signing, "reject").mockResolvedValue(undefined);
    vi.spyOn(store, "refreshWallet").mockResolvedValue(undefined);
    vi.spyOn(store, "refreshActivity").mockResolvedValue(undefined);
    vi.spyOn(store, "save").mockImplementation(() => {});
    onResolved = vi.fn();
    store.setState({ walletReview: { kind: "send", label: "Send", view, rawAck: false, onResolved } });
  });
  afterEach(() => {
    vi.restoreAllMocks();
    store.setState({ walletReview: null });
  });

  for (const [name, reason] of [
    ["no network set", NOT_SET],
    ["a retired network", RETIRED],
    ["an endpoint that could not be checked (#494)", UNREADABLE],
  ] as const) {
    it(`wallet review: ${name} reads "not sent" with the reason verbatim`, async () => {
      vi.spyOn(bridge.signing, "broadcast").mockRejectedValue("not submitted: " + reason);
      await store.approveWalletReview(false);
      expect(lastToast()).toBe("Send not sent — " + reason);
      expect(lastToast()).not.toMatch(/outcome unknown/i);
      expect(onResolved).toHaveBeenCalledWith(false);
    });
  }

  it("dApp provider: a refused personal_sign rejects with the gate's reason verbatim", async () => {
    const provider = createCeremonyProvider({
      origin: "https://app.example.org",
      chainId: 1,
      getAddress: async () => "0x" + "11".repeat(20),
      transportRequest: async () => null,
      approvalHook: async () => ({ rawAck: false }),
      signing: {
        request: async () => ({ ...view, kind: "personal_sign" }),
        approve: async () => {
          throw NOT_SET;
        },
        broadcast: async () => {
          throw new Error("unused");
        },
        reject: async () => {},
      } as never,
    });
    await expect(provider.request({ method: "personal_sign", params: ["0x68656c6c6f", "0x" + "11".repeat(20)] })).rejects.toBe(NOT_SET);
  });
});
