// =====================================================================
// citrate-core — B1.3 wagmi/EIP-1193 ceremony connector (D-13)
//
// The ONE signing route from JS. This connector wraps an internal EIP-1193
// provider so that every wagmi signing call (`personal_sign`,
// `eth_signTypedData_v4`, `eth_sendTransaction`) resolves ONLY by routing an
// intent through `bridge.signing` → the Rust SignatureCeremony → a human
// approval. Signing NEVER happens in JS:
//
//   • The connector holds no key and performs no crypto. It marshals a
//     SignatureIntent to the bridge and returns exactly the signature the
//     ceremony produced. (Rule 3 / I-2 — the gated signer lives in Rust.)
//   • Reads (eth_call, eth_chainId, eth_getBalance, eth_estimateGas, …) go
//     straight to the viem http transport for chain 40204. No ceremony.
//   • eth_accounts / eth_requestAccounts return the wallet ADDRESS from the
//     bridge wallet domain — never a private key.
//   • Rejection maps to the EIP-1193 user-rejected error (code 4001).
//   • eth_sendTransaction routes INTO the ceremony (so the human approves), then
//     — B1.4 — the vault key signs the REAL EIP-155 legacy tx in Rust and the
//     app broadcasts it to the live 40204 RPC, returning the REAL node-accepted
//     tx hash. It never fabricates a hash (Rule 1); if signing/broadcast fails
//     the underlying error propagates. (The B1.3 4900 deferral is removed.)
//   • `origin` in the intent is the TRUE caller origin, displayed verbatim by
//     the Rust ceremony (anti-spoof). The connector never sends a "benign" flag.
//
// The interactive approval UI is provided by an `approvalHook`: a function the
// host supplies that surfaces the pending CeremonyView to the human and returns
// their decision (approve with a raw-ack, or reject → null). B1.2 shipped the
// ceremony UI; wiring the hook to that live UI is a small host-side step and is
// intentionally left as a clear seam here (see `defaultApprovalHook`).
// =====================================================================
import { http, type Transport } from "viem";
import { createConnector } from "wagmi";
import { citrate } from "../chain";
import { bridge } from "../bridge";
import type { SigningDomain } from "../bridge/domains";
import type {
  SignatureIntent,
  IntentKind,
  CeremonyView,
} from "../bridge/types";

// ---------------------------------------------------------------------
// EIP-1193 error (subset). Code 4001 = user rejected request.
// ---------------------------------------------------------------------
export class ProviderRpcError extends Error {
  readonly code: number;
  readonly data?: unknown;
  constructor(code: number, message: string, data?: unknown) {
    super(message);
    this.name = "ProviderRpcError";
    this.code = code;
    this.data = data;
  }
}

/** EIP-1193 user-rejected-request error (code 4001). */
export function userRejectedError(detail?: string): ProviderRpcError {
  return new ProviderRpcError(
    4001,
    "User rejected the signing ceremony" + (detail ? ` — ${detail}` : ""),
  );
}

// ---------------------------------------------------------------------
// The methods the provider recognises, split by how they resolve.
// ---------------------------------------------------------------------

/** Methods that require the human-in-the-loop ceremony (mapped to intent kinds). */
const SIGNING_METHODS: Record<string, IntentKind> = {
  personal_sign: "personal_sign",
  eth_sign: "personal_sign",
  eth_signTypedData: "typed_data",
  eth_signTypedData_v4: "typed_data",
  eth_sendTransaction: "transaction",
};

/** Account methods → the wallet ADDRESS from the bridge (never a key). */
const ACCOUNT_METHODS = new Set(["eth_accounts", "eth_requestAccounts"]);

// ---------------------------------------------------------------------
// The approval hook. The provider does NOT decide approvals; a human does.
// The host wires this to the ceremony UI (B1.2). A `null` result = rejection.
// ---------------------------------------------------------------------
export interface ApprovalDecision {
  /** Must be true to approve undecodable calldata (view.requiresRawAck). */
  rawAck: boolean;
}
export type ApprovalHook = (view: CeremonyView) => Promise<ApprovalDecision | null>;

/**
 * Default approval hook. B1.3 does NOT ship the interactive approval UI (that
 * is a host-side wiring step onto the B1.2 ceremony UI). Rather than silently
 * auto-approving (which would defeat the human-in-the-loop invariant) or
 * fabricating a decision (Rule 1), the default HONESTLY refuses: it rejects the
 * ceremony. The host MUST supply a real `approvalHook` wired to the ceremony UI
 * for signing to succeed.
 */
export const defaultApprovalHook: ApprovalHook = async () => {
  // No UI wired → treat as a rejection (fail closed). Never auto-approve.
  return null;
};

// ---------------------------------------------------------------------
// The internal EIP-1193 provider.
// ---------------------------------------------------------------------
export interface CeremonyProviderDeps {
  /** The bridge signing domain (drives the Rust SignatureCeremony). */
  signing: SigningDomain;
  /** The viem transport's request fn for reads (chain 40204). */
  transportRequest: (args: { method: string; params?: unknown }) => Promise<unknown>;
  /** The wallet address from the bridge wallet domain (never a key). */
  getAddress: () => Promise<string | null>;
  /** Surfaces the pending ceremony to the human; null = rejection. */
  approvalHook: ApprovalHook;
  /** The chain id the intents target (40204). */
  chainId: number;
  /** The TRUE caller origin, displayed verbatim by the ceremony. */
  origin: string;
}

export interface CeremonyProvider {
  request(args: { method: string; params?: unknown }): Promise<unknown>;
}

/** Coerce a raw signing payload (personal_sign / typed data / tx) to a hex/JSON
 * string for the intent. No crypto — pure marshalling of the caller's payload. */
function payloadForKind(kind: IntentKind, params: unknown): string {
  const arr = Array.isArray(params) ? params : [];
  if (kind === "personal_sign") {
    // personal_sign params are [data, address]; eth_sign is [address, data].
    // We take the first hex-looking arg as the message payload.
    const first = arr[0];
    const second = arr[1];
    const pick = typeof first === "string" && first.startsWith("0x") ? first : second;
    return typeof pick === "string" ? pick : String(first ?? "");
  }
  if (kind === "typed_data") {
    // eth_signTypedData_v4 params are [address, typedDataJson].
    const td = arr[1] ?? arr[0];
    return typeof td === "string" ? td : JSON.stringify(td ?? {});
  }
  // transaction — the tx object, serialised for the ceremony to decode/display.
  const tx = arr[0];
  return typeof tx === "string" ? tx : JSON.stringify(tx ?? {});
}

/**
 * Build the internal EIP-1193 provider. This is the testable core: it routes
 * every method to reads / accounts / ceremony and NEVER signs in JS.
 */
export function createCeremonyProvider(deps: CeremonyProviderDeps): CeremonyProvider {
  /**
   * Drive request → human approval for `kind`/`params`, returning the pending
   * ceremony view id + the human's raw-ack once approved. Signs NOTHING itself;
   * the caller then either `approve`s (message/typed-data) or `broadcast`s (tx).
   * A rejection / errored approval consumes the ceremony and throws 4001.
   */
  async function requestAndAwaitApproval(
    kind: IntentKind,
    params: unknown,
  ): Promise<{ id: string; rawAck: boolean }> {
    const intent: SignatureIntent = {
      origin: deps.origin, // TRUE caller origin — never a caller-claimed flag.
      kind,
      chainId: deps.chainId,
      raw: payloadForKind(kind, params),
    };
    // 1) Submit the intent → a PENDING ceremony. This signs NOTHING.
    const view = await deps.signing.request(intent);
    // 2) Surface it to the human (ceremony UI, B1.2). Null = rejection.
    let decision: ApprovalDecision | null;
    try {
      decision = await deps.approvalHook(view);
    } catch (e) {
      // The approval surface errored — consume the pending ceremony, fail closed.
      await deps.signing.reject(view.id).catch(() => {});
      throw userRejectedError(e instanceof Error ? e.message : undefined);
    }
    if (!decision) {
      // Human rejected → consume the ceremony, surface EIP-1193 4001. No sig.
      await deps.signing.reject(view.id).catch(() => {});
      throw userRejectedError();
    }
    return { id: view.id, rawAck: decision.rawAck };
  }

  /** Message / typed-data path: approve the specific id → the ceremony's sig hex. */
  async function driveCeremony(kind: IntentKind, params: unknown): Promise<string> {
    const { id, rawAck } = await requestAndAwaitApproval(kind, params);
    // Approve the SPECIFIC id → the Rust ceremony produces the signature.
    const result = await deps.signing.approve(id, rawAck);
    return result.sigHex;
  }

  /** Transaction path (B1.4): approve the specific id → sign the real EIP-155 tx
   * with the vault key → broadcast to 40204 → return the REAL tx hash. */
  async function driveTransaction(params: unknown): Promise<string> {
    const { id, rawAck } = await requestAndAwaitApproval("transaction", params);
    const result = await deps.signing.broadcast(id, rawAck);
    return result.txHash;
  }

  return {
    async request(args: { method: string; params?: unknown }): Promise<unknown> {
      const { method, params } = args;

      // ---- accounts: the wallet ADDRESS from the bridge (never a key) ----
      if (ACCOUNT_METHODS.has(method)) {
        const addr = await deps.getAddress();
        return addr ? [addr] : [];
      }

      // ---- signing: route through the ceremony (human-in-the-loop) ----
      const kind = SIGNING_METHODS[method];
      if (kind) {
        if (method === "eth_sendTransaction") {
          // B1.4: the ceremony approves, the vault key signs the real EIP-155 tx,
          // and it is broadcast to 40204 — return the REAL tx hash the node
          // accepted (no more 4900 deferral; no fabricated hash — Rule 1).
          return driveTransaction(params);
        }
        // personal_sign / typed_data → return the ceremony's signature hex.
        return driveCeremony(kind, params);
      }

      // ---- everything else is a READ → straight to the transport. No ceremony ----
      return deps.transportRequest({ method, params });
    },
  };
}

// ---------------------------------------------------------------------
// The wagmi connector wrapping the internal provider.
// ---------------------------------------------------------------------

/** Resolve the wallet address from the bridge wallet domain (metadata only). */
async function bridgeAddress(): Promise<string | null> {
  try {
    const b = await bridge.wallet.balances();
    return b.address && b.address !== "0x" ? b.address : null;
  } catch {
    // The wallet domain may be Unavailable in this build (Rule 1) — no address.
    return null;
  }
}

/** Options for the ceremony connector (host wires the approval UI in here). */
export interface CeremonyConnectorOptions {
  /** Wire this to the B1.2 ceremony UI. Defaults to fail-closed (no auto-approve). */
  approvalHook?: ApprovalHook;
  /** Override the read transport (tests). Defaults to viem http() for 40204. */
  transport?: Transport;
}

/**
 * The custom wagmi connector. It exposes the internal EIP-1193 provider whose
 * `request` routes reads→transport, accounts→bridge address, signing→ceremony.
 * No key, no crypto in JS.
 */
export function ceremonyConnector(options: CeremonyConnectorOptions = {}) {
  const approvalHook = options.approvalHook ?? defaultApprovalHook;

  type Provider = CeremonyProvider;

  return createConnector<Provider>((config) => {
    // Instantiate the read transport once, bound to the Citrate chain.
    const transportFactory = options.transport ?? http();
    const transport = transportFactory({ chain: citrate });

    let provider: Provider | undefined;
    function getProviderSync(): Provider {
      if (!provider) {
        provider = createCeremonyProvider({
          signing: bridge.signing,
          transportRequest: (a) => transport.request(a as never),
          getAddress: bridgeAddress,
          approvalHook,
          chainId: citrate.id,
          // TRUE origin of the running app (verbatim to the ceremony).
          origin: typeof window !== "undefined" ? window.location.origin : "local-user",
        });
      }
      return provider;
    }

    return {
      id: "citrateCeremony",
      name: "Citrate (Signature Ceremony)",
      type: "citrateCeremony",

      async connect(parameters) {
        const addr = await bridgeAddress();
        const addrs = (addr ? [addr] : []) as readonly `0x${string}`[];
        const chainId: number = citrate.id;
        config.emitter.emit("connect", { accounts: addrs, chainId });
        const accounts = parameters?.withCapabilities
          ? addrs.map((address) => ({ address, capabilities: {} }))
          : addrs;
        return { accounts, chainId } as never;
      },

      async disconnect() {
        provider = undefined;
        config.emitter.emit("disconnect");
      },

      async getAccounts() {
        const addr = await bridgeAddress();
        return (addr ? [addr] : []) as readonly `0x${string}`[];
      },

      async getChainId() {
        return citrate.id;
      },

      async getProvider() {
        return getProviderSync();
      },

      async isAuthorized() {
        // Authorized when the bridge exposes a wallet address.
        return (await bridgeAddress()) !== null;
      },

      onAccountsChanged(accounts) {
        if (accounts.length === 0) this.onDisconnect();
        else
          config.emitter.emit("change", {
            accounts: accounts as readonly `0x${string}`[],
          });
      },

      onChainChanged(chainId) {
        config.emitter.emit("change", { chainId: Number(chainId) });
      },

      onDisconnect() {
        provider = undefined;
        config.emitter.emit("disconnect");
      },
    };
  });
}
