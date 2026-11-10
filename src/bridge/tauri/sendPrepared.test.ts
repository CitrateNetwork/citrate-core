// Join-8 — sign_send_prepared returns a STRUCTURED error; the bridge maps only the
// "signedTxExpired" code to SignedTxExpired. Text never decides (a node error after a real
// send can quote the expiry sentence).
import { describe, it, expect, vi } from "vitest";

const invokeMock = vi.fn();
vi.mock("./invoke", () => ({ invoke: (...a: unknown[]) => invokeMock(...a), deadlineFor: () => 12_000 }));

import { createTauriBridge } from "./index";
import { SignedTxExpired } from "../types";

const SENTENCE = "ceremony: the signed transaction expired before it was sent; nothing was sent";

describe("signing.sendPrepared error mapping", () => {
  it("maps the structured signedTxExpired code to SignedTxExpired", async () => {
    invokeMock.mockRejectedValueOnce({ code: "signedTxExpired", message: SENTENCE });
    const err = await createTauriBridge().signing.sendPrepared("7").catch((e: unknown) => e);
    expect(err).toBeInstanceOf(SignedTxExpired);
    expect((err as Error).message).toBe(SENTENCE);
  });

  it("a failed send that QUOTES the sentence is not SignedTxExpired", async () => {
    invokeMock.mockRejectedValueOnce({ code: "failed", message: "ceremony: broadcast failed: rpc error -32000: " + SENTENCE });
    const err = await createTauriBridge().signing.sendPrepared("7").catch((e: unknown) => e);
    expect(err).not.toBeInstanceOf(SignedTxExpired);
    expect(err).toBeInstanceOf(Error);
    expect((err as Error).message).toContain("broadcast failed");
  });

  it("a bare string error (e.g. an IPC failure) is never SignedTxExpired", async () => {
    invokeMock.mockRejectedValueOnce(SENTENCE);
    const err = await createTauriBridge().signing.sendPrepared("7").catch((e: unknown) => e);
    expect(err).not.toBeInstanceOf(SignedTxExpired);
  });
});
