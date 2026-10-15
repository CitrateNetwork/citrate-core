// HGW-W11: Settings > Hermes · building. The switch writes the member's choice (and reports a
// restart), the list shown is the running Hermes's own, the identity is saved together or cleared
// together, and the web preview offers no controls.
import { describe, it, expect, vi } from "vitest";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { BuildSettingsPanel } from "./BuildSettingsPanel";
import { ruleText, type AutoApproveListView, type BuildSettingsIo, type BuildSettingsStatus } from "./buildSettings";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const ST: BuildSettingsStatus = { autoApprove: true, gitName: null, gitEmail: null, restarted: false, loadError: null };
const LIST: AutoApproveListView = {
  enabled: true,
  conditions: ["the session has not read untrusted content", "the OS sandbox is enforced (no network)"],
  rules: [
    { id: "npm-run", program: "npm", subcommands: ["run"], operands: "script-name", operand_values: ["build", "test", "lint"] },
    { id: "cargo", program: "cargo", subcommands: ["build"], require_one_of: ["--offline", "--frozen"], operands: "none" },
  ],
};

type Calls = Array<[string, Record<string, unknown> | undefined]>;
function makeIo(handler: (cmd: string, args?: Record<string, unknown>) => unknown, mode: "tauri" | "sim" = "tauri") {
  const calls: Calls = [];
  const io: BuildSettingsIo = {
    mode,
    invoke: async <T,>(cmd: string, args?: Record<string, unknown>) => {
      calls.push([cmd, args]);
      const r = handler(cmd, args);
      if (r instanceof Error) throw r;
      return r as T;
    },
  };
  return { io, calls };
}

async function mount(el: React.ReactElement): Promise<HTMLDivElement> {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(el);
  });
  await act(async () => {});
  await act(async () => {});
  return host;
}

function setInput(el: HTMLInputElement, value: string) {
  const set = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")?.set;
  set?.call(el, value);
  el.dispatchEvent(new Event("input", { bubbles: true }));
}

describe("ruleText", () => {
  it("reads like the command a person would type", () => {
    expect(ruleText(LIST.rules[0])).toBe("npm run build|test|lint");
    expect(ruleText(LIST.rules[1])).toBe("cargo build (with --offline or --frozen)");
    expect(ruleText({ id: "git-add", program: "git", subcommands: ["add"], operands: "relative-path" })).toBe("git add <relative-path>");
  });
});

describe("BuildSettingsPanel", () => {
  it("in the web preview offers no controls and asks core for nothing", async () => {
    const { io, calls } = makeIo(() => ST, "sim");
    const host = await mount(<BuildSettingsPanel io={async () => io} />);
    expect(host.textContent).toContain("desktop app");
    expect(host.querySelector("#hermes-auto-approve")).toBeNull();
    expect(calls).toEqual([]);
  });

  it("shows the stored switch and the running Hermes's list with its conditions", async () => {
    const { io } = makeIo((cmd) => (cmd === "hermes_auto_approve_list" ? LIST : ST));
    const host = await mount(<BuildSettingsPanel io={async () => io} />);
    expect((host.querySelector("#hermes-auto-approve") as HTMLInputElement).checked).toBe(true);
    const list = host.querySelector('[data-testid="auto-approve-list"]');
    expect(list?.textContent).toContain("npm run build|test|lint");
    expect(list?.textContent).toContain("no network");
  });

  it("says when the list is not available instead of showing one", async () => {
    const { io } = makeIo((cmd) => (cmd === "hermes_auto_approve_list" ? new Error("Hermes is not running; start it to see the list") : ST));
    const host = await mount(<BuildSettingsPanel io={async () => io} />);
    expect(host.querySelector('[data-testid="auto-approve-list"]')).toBeNull();
    expect(host.textContent).toContain("Hermes is not running");
  });

  it("turning it off saves the choice and reports the restart", async () => {
    const restarted = vi.fn();
    const { io, calls } = makeIo((cmd, args) => {
      if (cmd === "hermes_auto_approve_list") return { ...LIST, enabled: false, rules: [] };
      if (cmd === "hermes_build_settings_set") return { ...ST, autoApprove: args?.autoApprove as boolean, restarted: true };
      return ST;
    });
    const host = await mount(<BuildSettingsPanel io={async () => io} onRestarted={restarted} />);
    await act(async () => {
      (host.querySelector("#hermes-auto-approve") as HTMLInputElement).click();
    });
    await act(async () => {});
    expect(calls.find(([c]) => c === "hermes_build_settings_set")?.[1]).toEqual({ autoApprove: false, gitName: null, gitEmail: null });
    expect(restarted).toHaveBeenCalledTimes(1);
    expect(host.textContent).toContain("Hermes restarted");
    expect(host.textContent).toContain("every command asks you");
  });

  it("saves the commit identity, and shows core's refusal of a half identity", async () => {
    const { io, calls } = makeIo((cmd, args) => {
      if (cmd === "hermes_auto_approve_list") return LIST;
      if (cmd === "hermes_build_settings_set") {
        if (!args?.gitEmail) return new Error("set both the name and the email, or clear both");
        return { ...ST, gitName: args.gitName, gitEmail: args.gitEmail };
      }
      return ST;
    });
    const host = await mount(<BuildSettingsPanel io={async () => io} />);
    const [name, email] = Array.from(host.querySelectorAll("input:not([type=checkbox])")) as HTMLInputElement[];
    const save = Array.from(host.querySelectorAll("button")).find((b) => b.textContent === "Save") as HTMLButtonElement;
    await act(async () => setInput(name, "Ada"));
    await act(async () => save.click());
    await act(async () => {});
    expect(host.textContent).toContain("set both the name and the email");
    await act(async () => setInput(email, "ada@example.org"));
    await act(async () => save.click());
    await act(async () => {});
    expect(calls.filter(([c]) => c === "hermes_build_settings_set").pop()?.[1]).toEqual({ autoApprove: true, gitName: "Ada", gitEmail: "ada@example.org" });
    expect(host.textContent).toContain("Saved.");
  });

  it("an unreadable settings file is shown, and the switch reads off", async () => {
    const { io } = makeIo((cmd) => (cmd === "hermes_auto_approve_list" ? LIST : { ...ST, autoApprove: false, loadError: "the Hermes building settings are not valid; auto-approve stays off until you set them again" }));
    const host = await mount(<BuildSettingsPanel io={async () => io} />);
    expect(host.textContent).toContain("auto-approve stays off");
    expect((host.querySelector("#hermes-auto-approve") as HTMLInputElement).checked).toBe(false);
  });
});
