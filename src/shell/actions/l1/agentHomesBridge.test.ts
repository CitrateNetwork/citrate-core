// L1-WP14b (planset v060): `bridge.acp` sign-in members. The tauri impl invokes exactly the three
// registered L1 commands with the agent id and nothing else; the browser preview answers
// `Unavailable` for each (no invented sign-in state, Rule 1).
import { afterEach, describe, expect, it, vi } from "vitest";

afterEach(() => {
  vi.doUnmock("../../../bridge/tauri/invoke");
  vi.resetModules();
});

describe("L1-WP14b bridge.acp agent homes", () => {
  it("tauri_impl_invokes_the_registered_commands_with_only_the_id", async () => {
    const calls: Array<{ cmd: string; args: unknown }> = [];
    vi.doMock("../../../bridge/tauri/invoke", () => ({
      invoke: async (cmd: string, args?: unknown) => {
        calls.push({ cmd, args });
        return cmd === "agent_runtime_remove"
          ? true
          : { id: "codex", homeExists: true, ownLoginPresent: false, ownLoginOffered: true };
      },
    }));
    const { tauriAcp } = await import("../../../bridge/tauri/l1");
    await tauriAcp.authStatus("codex");
    await tauriAcp.signOut("codex");
    expect(await tauriAcp.removeAgent("codex")).toBe(true);
    expect(calls).toEqual([
      { cmd: "agent_runtime_auth_status", args: { id: "codex" } },
      { cmd: "agent_runtime_sign_out", args: { id: "codex" } },
      { cmd: "agent_runtime_remove", args: { id: "codex" } },
    ]);
  });

  it("sim_impl_answers_unavailable_for_every_member", async () => {
    const { simAcp } = await import("../../../bridge/sim/l1");
    const { isUnavailable } = await import("../../../bridge/types");
    const acp = simAcp({} as never);
    for (const call of [() => acp.authStatus("codex"), () => acp.signOut("codex"), () => acp.removeAgent("codex")]) {
      const err = await call().then(
        () => null,
        (e: unknown) => e,
      );
      expect(isUnavailable(err)).toBe(true);
    }
  });

  it("the_registered_commands_and_the_l1_acl_agree_with_the_bridge", async () => {
    const { readFileSync } = await import("node:fs");
    const { join, dirname } = await import("node:path");
    const { fileURLToPath } = await import("node:url");
    const root = join(dirname(fileURLToPath(import.meta.url)), "..", "..", "..", "..");
    const list = readFileSync(join(root, "src-tauri/src/commands/l1.rs"), "utf8");
    const acl = readFileSync(join(root, "src-tauri/permissions/l1-commands.toml"), "utf8");
    for (const cmd of ["agent_runtime_auth_status", "agent_runtime_sign_out", "agent_runtime_remove"]) {
      expect(list).toContain(`hermes::agent_auth::${cmd},`);
      expect(acl).toContain(`"${cmd}",`);
    }
  });
});
