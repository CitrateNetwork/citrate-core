// =====================================================================
// citrate-core — B1.3 ceremony connector tests (D-13)
//
// RED-TEST-FIRST. These assert the ONE invariant of the ceremony connector:
// signing NEVER happens in JS. The internal EIP-1193 provider only marshals a
// SignatureIntent to the bridge's `signing` domain (which drives the Rust
// SignatureCeremony) and returns what the ceremony produced. It holds no key
// and performs no crypto. Reads bypass the ceremony straight to the viem
// transport. Rejection maps to EIP-1193 4001. `eth_sendTransaction` routes
// through the ceremony then broadcasts the real signed tx to 40204 (B1.4),
// returning the real node-accepted tx hash (never a fabricated hash).
// =====================================================================
import { describe, it, expect, vi } from "vitest";
import {
  createCeremonyProvider,
  ProviderRpcError,
  type CeremonyProviderDeps,
} from "./ceremonyConnector";
import type {
  SignatureIntent,
  CeremonyView,
  Signature,
  BroadcastResult,
} from "../bridge/types";

// A mock signing domain whose approve is only reached via an explicit approval.
// The `approve` spy proves the ceremony round-trip is required for a signature.
function mockSigning(opts?: {
  requiresRawAck?: boolean;
  sigHex?: string;
  txHash?: string;
  blockNumber?: number | null;
}) {
  const requiresRawAck = opts?.requiresRawAck ?? false;
  const requestSpy = vi.fn(async (intent: SignatureIntent): Promise<CeremonyView> => ({
    id: "ceremony-1",
    origin: intent.origin,
    kind: intent.kind,
    chainId: intent.chainId,
    decoded: { action: "Sign message", cost: "no funds moved", destination: intent.origin },
    requiresRawAck,
  }));
  const approveSpy = vi.fn(async (id: string, rawAck: boolean): Promise<Signature> => {
    if (id !== "ceremony-1") throw new Error("unknown id");
    return { sigHex: opts?.sigHex ?? "0xdeadbeef", kind: "personal_sign" };
  });
  // B1.4: the transaction path calls broadcast → real tx hash + block.
  const broadcastSpy = vi.fn(async (id: string, _rawAck: boolean): Promise<BroadcastResult> => {
    if (id !== "ceremony-1") throw new Error("unknown id");
    return {
      txHash:
        opts?.txHash ??
        "0xabc0000000000000000000000000000000000000000000000000000000000abc",
      blockNumber: opts?.blockNumber ?? 100,
    };
  });
  const rejectSpy = vi.fn(async (_id: string): Promise<void> => {});
  return {
    domain: { request: requestSpy, approve: approveSpy, broadcast: broadcastSpy, reject: rejectSpy },
    requestSpy,
    approveSpy,
    broadcastSpy,
    rejectSpy,
  };
}

// A read transport spy — reads MUST hit this, never the ceremony.
function mockTransport(result: unknown = "0x1") {
  const spy = vi.fn(async (_args: { method: string; params?: unknown }) => result);
  return { request: spy, spy };
}

function deps(over: Partial<CeremonyProviderDeps> = {}): CeremonyProviderDeps {
  const { domain } = mockSigning();
  const { request } = mockTransport();
  return {
    signing: domain,
    transportRequest: request,
    getAddress: async () => "0xabc0000000000000000000000000000000000001",
    // Default approval hook: approve with the required rawAck.
    approvalHook: async (view: CeremonyView) => ({ rawAck: view.requiresRawAck }),
    chainId: 40204,
    origin: "https://app.citrate.ai",
    ...over,
  };
}

describe("B1.3 ceremony connector — signing routes through the bridge only", () => {
  // B1.3-ADV-1: personal_sign cannot resolve without a ceremony approval.
  it("B1.3-ADV-1: personal_sign does NOT resolve if approval is never granted", async () => {
    const { domain, approveSpy } = mockSigning();
    // Approval hook that refuses (models a user who never clicks approve).
    const provider = createCeremonyProvider(
      deps({ signing: domain, approvalHook: async () => null }),
    );
    await expect(
      provider.request({ method: "personal_sign", params: ["0x68656c6c6f", "0xabc"] }),
    ).rejects.toBeInstanceOf(ProviderRpcError);
    // The bridge approve was NEVER called → no signature could have been produced.
    expect(approveSpy).not.toHaveBeenCalled();
  });

  // B1.3-ADV-3: rejection → EIP-1193 4001, no signature returned.
  it("B1.3-ADV-3: a rejected ceremony throws EIP-1193 4001 and yields no signature", async () => {
    const { domain, approveSpy, rejectSpy } = mockSigning();
    const provider = createCeremonyProvider(
      deps({ signing: domain, approvalHook: async () => null }),
    );
    let caught: unknown;
    try {
      await provider.request({ method: "personal_sign", params: ["0x68656c6c6f", "0xabc"] });
    } catch (e) {
      caught = e;
    }
    expect(caught).toBeInstanceOf(ProviderRpcError);
    expect((caught as ProviderRpcError).code).toBe(4001);
    expect(approveSpy).not.toHaveBeenCalled();
    // The rejected ceremony is consumed via reject (no dangling pending id).
    expect(rejectSpy).toHaveBeenCalledWith("ceremony-1");
  });

  // B1.3-ADV-4: reads do NOT trigger a ceremony (straight to transport).
  it("B1.3-ADV-4: eth_call and eth_chainId bypass the ceremony to the transport", async () => {
    const { domain, requestSpy, approveSpy } = mockSigning();
    const t = mockTransport("0xcafe");
    const provider = createCeremonyProvider(
      deps({ signing: domain, transportRequest: t.request }),
    );
    const call = await provider.request({ method: "eth_call", params: [{ to: "0x0" }] });
    const cid = await provider.request({ method: "eth_chainId" });
    expect(call).toBe("0xcafe");
    expect(cid).toBe("0xcafe");
    // Two reads → two transport hits, ZERO ceremony interaction.
    expect(t.spy).toHaveBeenCalledTimes(2);
    expect(requestSpy).not.toHaveBeenCalled();
    expect(approveSpy).not.toHaveBeenCalled();
  });

  // Negative control for ADV-4: a signing method DOES trigger the ceremony and
  // does NOT hit the read transport — proving the read/sign split is real.
  it("B1.3-ADV-4 (negative control): personal_sign hits the ceremony, not the transport", async () => {
    const { domain, requestSpy } = mockSigning();
    const t = mockTransport("0xshould-not-be-used");
    const provider = createCeremonyProvider(
      deps({ signing: domain, transportRequest: t.request }),
    );
    await provider.request({ method: "personal_sign", params: ["0x68656c6c6f", "0xabc"] });
    expect(requestSpy).toHaveBeenCalledTimes(1);
    expect(t.spy).not.toHaveBeenCalled();
  });

  // B1.4: eth_sendTransaction routes through the ceremony → the vault key signs
  // the real EIP-155 tx → broadcast → the REAL node-accepted tx hash is returned.
  it("B1.4: eth_sendTransaction routes through the ceremony then broadcasts, returning the real tx hash", async () => {
    const realHash = "0xdeadbeef00000000000000000000000000000000000000000000000000000abc";
    const { domain, requestSpy, broadcastSpy, approveSpy } = mockSigning({
      requiresRawAck: true,
      txHash: realHash,
      blockNumber: 4242,
    });
    const provider = createCeremonyProvider(deps({ signing: domain }));
    const result = await provider.request({
      method: "eth_sendTransaction",
      params: [{ to: "0xdead", value: "0x1" }],
    });
    // The wagmi caller gets the REAL tx hash (not a signature, not a fake hash).
    expect(result).toBe(realHash);
    // The tx path drives request → broadcast (NOT the message-only approve).
    expect(requestSpy).toHaveBeenCalledTimes(1);
    expect(broadcastSpy).toHaveBeenCalledTimes(1);
    expect(broadcastSpy).toHaveBeenCalledWith("ceremony-1", true); // rawAck passed through
    expect(approveSpy).not.toHaveBeenCalled();
  });

  // B1.4 negative control: a rejected tx ceremony never broadcasts (no tx hash).
  it("B1.4: a rejected eth_sendTransaction throws 4001 and never broadcasts", async () => {
    const { domain, broadcastSpy, rejectSpy } = mockSigning();
    const provider = createCeremonyProvider(deps({ signing: domain, approvalHook: async () => null }));
    let caught: unknown;
    try {
      await provider.request({ method: "eth_sendTransaction", params: [{ to: "0xdead", value: "0x1" }] });
    } catch (e) {
      caught = e;
    }
    expect(caught).toBeInstanceOf(ProviderRpcError);
    expect((caught as ProviderRpcError).code).toBe(4001);
    expect(broadcastSpy).not.toHaveBeenCalled();
    expect(rejectSpy).toHaveBeenCalledWith("ceremony-1");
  });

  // B1.3-INT: request → view → approve → the caller receives the ceremony sig.
  it("B1.3-INT: request → ceremony view → approve → returns the ceremony's signature", async () => {
    const { domain, requestSpy, approveSpy } = mockSigning({ sigHex: "0xfeedface" });
    let seenView: CeremonyView | null = null;
    const provider = createCeremonyProvider(
      deps({
        signing: domain,
        approvalHook: async (view) => {
          seenView = view;
          return { rawAck: view.requiresRawAck };
        },
      }),
    );
    const sig = await provider.request({
      method: "personal_sign",
      params: ["0x68656c6c6f", "0xabc"],
    });
    // The wagmi caller gets exactly the ceremony's signature hex.
    expect(sig).toBe("0xfeedface");
    expect(seenView).not.toBeNull();
    expect(seenView!.id).toBe("ceremony-1");
    // The intent's origin was the TRUE caller origin.
    expect(requestSpy).toHaveBeenCalledWith(
      expect.objectContaining({ origin: "https://app.citrate.ai", kind: "personal_sign", chainId: 40204 }),
    );
    expect(approveSpy).toHaveBeenCalledWith("ceremony-1", false);
  });

  it("eth_signTypedData_v4 routes through the ceremony and returns its signature", async () => {
    const { domain, requestSpy } = mockSigning({ sigHex: "0xabcdef" });
    const provider = createCeremonyProvider(deps({ signing: domain, approvalHook: async (v) => ({ rawAck: v.requiresRawAck }) }));
    const sig = await provider.request({
      method: "eth_signTypedData_v4",
      params: ["0xabc", '{"types":{}}'],
    });
    expect(sig).toBe("0xabcdef");
    expect(requestSpy).toHaveBeenCalledWith(
      expect.objectContaining({ kind: "typed_data", chainId: 40204 }),
    );
  });

  it("eth_accounts / eth_requestAccounts return the bridge wallet address, never a key", async () => {
    const provider = createCeremonyProvider(
      deps({ getAddress: async () => "0xABC0000000000000000000000000000000000001" }),
    );
    const a1 = (await provider.request({ method: "eth_accounts" })) as string[];
    const a2 = (await provider.request({ method: "eth_requestAccounts" })) as string[];
    expect(a1).toEqual(["0xABC0000000000000000000000000000000000001"]);
    expect(a2).toEqual(["0xABC0000000000000000000000000000000000001"]);
  });

  it("eth_accounts returns [] honestly when no wallet address is available", async () => {
    const provider = createCeremonyProvider(deps({ getAddress: async () => null }));
    const a = (await provider.request({ method: "eth_accounts" })) as string[];
    expect(a).toEqual([]);
  });

  it("raw-ack gated ceremonies pass the hook's ack through to approve", async () => {
    const { domain, approveSpy } = mockSigning({ requiresRawAck: true, sigHex: "0x11" });
    const provider = createCeremonyProvider(
      deps({ signing: domain, approvalHook: async (v) => ({ rawAck: v.requiresRawAck }) }),
    );
    const sig = await provider.request({ method: "personal_sign", params: ["0xdead", "0xabc"] });
    expect(sig).toBe("0x11");
    expect(approveSpy).toHaveBeenCalledWith("ceremony-1", true);
  });
});

// B1.3-ADV-2: source scan — NO signing / private-key / crypto logic in src/.
// The connector delegates every signing decision to the bridge; nothing in the
// JS source signs, holds a key, or does secp256k1 / keystore work.
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const SRC_ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");

function walkTs(dir: string, out: string[] = []): string[] {
  for (const name of readdirSync(dir)) {
    if (name === "node_modules") continue;
    const p = join(dir, name);
    const st = statSync(p);
    if (st.isDirectory()) walkTs(p, out);
    else if ((name.endsWith(".ts") || name.endsWith(".tsx")) && !name.endsWith(".test.ts") && !name.endsWith(".test.tsx")) {
      out.push(p);
    }
  }
  return out;
}

// Strip comments and string/template literals so the scan targets executable
// CODE, not UI copy or explanatory prose. A signing invariant is about what the
// JS DOES, not what its labels say (the app's UI legitimately describes that the
// real keystore/keyring lives in Rust). This keeps the scan robust, not flaky.
function stripCommentsAndStrings(src: string): string {
  return src
    // block comments
    .replace(/\/\*[\s\S]*?\*\//g, " ")
    // line comments
    .replace(/(^|[^:])\/\/[^\n]*/g, "$1 ")
    // template / single / double quoted literals
    .replace(/`(?:\\.|[^`\\])*`/g, '""')
    .replace(/'(?:\\.|[^'\\])*'/g, '""')
    .replace(/"(?:\\.|[^"\\])*"/g, '""');
}

describe("B1.3-ADV-2: source scan — no signing/private-key/crypto lives in JS", () => {
  // Patterns that would indicate executable JS is signing / holding keys / doing
  // crypto. These operations MUST live only in Rust (the ceremony), so any real
  // occurrence in code (not comments/strings) is a violation.
  const FORBIDDEN: { re: RegExp; label: string }[] = [
    { re: /\bsecp256k1\b/i, label: "secp256k1 curve op" },
    { re: /\bprivateKeyTo(Account|Address)\b/, label: "viem privateKey→account" },
    { re: /\bmnemonicTo(Account|Seed)\b/, label: "mnemonic derivation" },
    { re: /\bhdKeyToAccount\b/, label: "HD key derivation" },
    // A .sign( / signMessage( / signTransaction( / signTypedData( call in code,
    // but NOT Math.sign (a numeric sign, not a crypto signer).
    { re: /(?<!Math)\.sign\s*\(/, label: "a .sign( call" },
    { re: /\bsignMessage\s*\(/, label: "signMessage(" },
    { re: /\bsignTransaction\s*\(/, label: "signTransaction(" },
    { re: /\bsignTypedData\s*\(/, label: "signTypedData(" },
    { re: /@noble\/(curves|hashes\/secp)/, label: "noble curve import" },
    { re: /\bfrom\s*""privateKey/, label: "private-key import" },
  ];

  it("no forbidden signing/key/crypto operations appear in any src/ .ts(x) code", () => {
    const files = walkTs(SRC_ROOT);
    // Sanity: the walk found real files (guards against a broken scan).
    expect(files.length).toBeGreaterThan(5);
    const hits: string[] = [];
    for (const f of files) {
      const code = stripCommentsAndStrings(readFileSync(f, "utf8"));
      for (const { re, label } of FORBIDDEN) {
        if (re.test(code)) hits.push(`${f}: ${label}`);
      }
    }
    expect(hits).toEqual([]);
  });

  // Negative control: the scan CAN detect a real violation (guards against a
  // scan that passes because its patterns are broken / too narrow).
  it("(negative control) the scan flags a synthetic signing/key operation", () => {
    const malicious = [
      "import { privateKeyToAccount } from 'viem/accounts';",
      "const acct = privateKeyToAccount('0x...');",
      "const sig = await acct.signMessage({ message: 'x' });",
    ].join("\n");
    const code = stripCommentsAndStrings(malicious);
    const anyHit = FORBIDDEN.some(({ re }) => re.test(code));
    expect(anyHit).toBe(true);
  });
});
