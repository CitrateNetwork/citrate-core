// CORE-A2 A2.5 — custody bridge domain. The frontend half of the vault:
//  - Tauri adapter invokes the real custody commands and NEVER a command that
//    returns secret bytes (ADV-8: the invoke surface is status + metadata only).
//  - Sim adapter simulates lock/unlock UI STATE ONLY — no real secret, stores
//    nothing — and is guarded out of packaged builds by assertSimAllowed.
import { describe, it, expect, vi, beforeEach } from "vitest";

// ---- Tauri adapter: mock the invoke boundary ------------------------------
const invoked: { cmd: string; args?: Record<string, unknown> }[] = [];
const invokeMock = vi.fn(async (cmd: string, args?: Record<string, unknown>) => {
  invoked.push({ cmd, args });
  switch (cmd) {
    case "custody_status":
      return { initialized: true, unlocked: false, autolockMins: 30, keyringStatus: "available" };
    case "custody_init":
    case "custody_unlock":
    case "custody_lock":
      return null;
    case "custody_list":
      return [{ name: "oidc-refresh", bytes: 64 }];
    default:
      throw `unexpected command ${cmd}`;
  }
});

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (cmd: string, args?: Record<string, unknown>) => invokeMock(cmd, args),
  isTauri: () => true,
}));

import { createTauriBridge } from "./tauri";

describe("custody bridge — tauri adapter invokes the real A2 commands", () => {
  beforeEach(() => {
    invoked.length = 0;
    invokeMock.mockClear();
  });

  it("status reads real custody_status (initialized/unlocked/autolock/keyring)", async () => {
    const b = createTauriBridge();
    const st = await b.custody.status();
    expect(st.initialized).toBe(true);
    expect(st.unlocked).toBe(false);
    expect(st.autolockMins).toBe(30);
    expect(st.keyringStatus).toBe("available");
    expect(invokeMock).toHaveBeenCalledWith("custody_status", undefined);
  });

  it("init/unlock/lock invoke the matching commands; passphrase passed as arg", async () => {
    const b = createTauriBridge();
    await b.custody.init("s3cret-pass");
    await b.custody.unlock("s3cret-pass");
    await b.custody.lock();
    expect(invokeMock).toHaveBeenCalledWith("custody_init", { passphrase: "s3cret-pass" });
    expect(invokeMock).toHaveBeenCalledWith("custody_unlock", { passphrase: "s3cret-pass" });
    expect(invokeMock).toHaveBeenCalledWith("custody_lock", undefined);
  });

  it("listSlots returns metadata only — never secret bytes", async () => {
    const b = createTauriBridge();
    const slots = await b.custody.listSlots();
    expect(slots).toEqual([{ name: "oidc-refresh", bytes: 64 }]);
    // metadata shape carries no secret payload field
    for (const slot of slots) {
      expect(Object.keys(slot).sort()).toEqual(["bytes", "name"]);
    }
  });

  it("ADV-8 boundary: the custody domain exposes no secret-read op, and no invoked command is a getter", async () => {
    const b = createTauriBridge();
    // The domain surface has no `get`/`read`-secret method.
    expect((b.custody as Record<string, unknown>).get).toBeUndefined();
    // Drive every custody op and assert no invoked command name reads a secret.
    await b.custody.status();
    await b.custody.init("p");
    await b.custody.unlock("p");
    await b.custody.lock();
    await b.custody.listSlots();
    const names = invoked.map((i) => i.cmd);
    expect(names).not.toContain("custody_get");
    // Every invoked command is a status/metadata/void op.
    const allowed = new Set([
      "custody_status",
      "custody_init",
      "custody_unlock",
      "custody_lock",
      "custody_list",
    ]);
    for (const n of names) expect(allowed.has(n)).toBe(true);
  });
});
