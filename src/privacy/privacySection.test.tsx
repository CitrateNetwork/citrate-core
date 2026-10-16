// HUP-S10.5 — the Privacy & recovery panels and the telemetry consent screen.
// Written alongside the components, then mutation-checked (dropping the confirmation gate or a
// consent field makes these fail).
import { describe, it, expect, vi } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { renderToStaticMarkup } from "react-dom/server";
import type { PrivacyIo } from "./privacyIo";
import { PrivacySection, DeleteLocalDataPanel, RecoveryKitPanel } from "./PrivacySection";
import { TelemetryConsent } from "./TelemetryConsent";
import matrix from "./offline-matrix.json";
import fields from "./telemetry-fields.json";
import { freshState } from "../shell/state";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const status = {
  keyringReachable: true,
  keys: [
    { account: "node-storage-key", label: "Node storage key", covers: "the node's encrypted chain data on this computer", present: true, fingerprint: "aaaabbbbccccdddd", recordedFingerprint: null },
    { account: "memory-store-key", label: "Journal and memory key", covers: "the memory store", present: false, fingerprint: null, recordedFingerprint: null },
  ],
};
const plan = {
  options: { includeWallet: false, keepModels: false },
  entries: [
    { path: "/d/ai.citrate.core/custody.enc", kind: "data", bytes: 5, action: "keep", reason: "your wallet vault (kept unless you also delete the wallet)" },
    { path: "/d/ai.citrate.core/node", kind: "data", bytes: 2048, action: "delete", reason: null },
  ],
  keychain: [{ service: "ai.citrate.core", account: "node-storage-key", label: "Node storage key", present: true, action: "delete" }],
  deleteBytes: 2048,
  keepBytes: 5,
  confirmPhrase: "delete my local data",
  walletConfirmPhrase: null,
  notes: [],
};

function makeIo(invoke: (cmd: string, args: Record<string, unknown>) => unknown, over: Partial<PrivacyIo> = {}): () => Promise<PrivacyIo> {
  const io: PrivacyIo = {
    mode: "tauri",
    pickSavePath: async () => "/Users/me/kit",
    pickOpenPath: async () => "/Users/me/kit.citrate-recovery",
    invoke: (async (cmd: string, args: Record<string, unknown>) => invoke(cmd, args)) as PrivacyIo["invoke"],
    clearWebStorage: vi.fn(),
    ...over,
  };
  return async () => io;
}

async function mount(el: React.ReactElement): Promise<{ host: HTMLDivElement; root: Root }> {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(el);
  });
  await act(async () => {});
  return { host, root };
}
const q = (host: HTMLElement, id: string) => host.querySelector(`[data-testid="${id}"]`) as HTMLElement | null;
async function click(el: Element | null) {
  expect(el).toBeTruthy();
  await act(async () => {
    (el as HTMLElement).click();
  });
  await act(async () => {});
}
async function type(el: Element | null, value: string) {
  expect(el).toBeTruthy();
  const proto = el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
  const setter = Object.getOwnPropertyDescriptor(proto, "value")?.set;
  await act(async () => {
    setter?.call(el, value);
    el!.dispatchEvent(new Event("input", { bubbles: true }));
  });
}

describe("telemetry consent", () => {
  it("is off by default in a fresh install", () => {
    expect(freshState("p1").telemetry).toBe(false);
  });

  it("turning on goes through a screen that lists every field sent and what is never sent", async () => {
    const onChange = vi.fn();
    const { host } = await mount(<TelemetryConsent enabled={false} onChange={onChange} />);
    expect(q(host, "telemetry-state")!.textContent).toContain("Off");
    expect(q(host, "telemetry-consent-on")).toBeNull();
    await click(q(host, "telemetry-review"));
    const screen = q(host, "telemetry-consent-screen")!;
    for (const f of fields.fields) {
      expect(screen.textContent).toContain(f.key);
      expect(screen.textContent).toContain(f.what);
    }
    for (const n of fields.neverSent) expect(screen.textContent).toContain(n);
    expect(screen.textContent).toContain(fields.endpoint);
    await click(q(host, "telemetry-consent-close"));
    expect(onChange).not.toHaveBeenCalled();
    await click(q(host, "telemetry-review"));
    await click(q(host, "telemetry-consent-on"));
    expect(onChange).toHaveBeenCalledWith(true);
  });

  it("can be turned off in one click", async () => {
    const onChange = vi.fn();
    const { host } = await mount(<TelemetryConsent enabled={true} onChange={onChange} />);
    await click(q(host, "telemetry-off"));
    expect(onChange).toHaveBeenCalledWith(false);
  });
});

describe("recovery kit panel", () => {
  it("shows each key without key bytes and says the wallet is not covered", async () => {
    const { host } = await mount(<RecoveryKitPanel io={makeIo(() => status)} now={() => new Date()} toast={() => {}} />);
    const keys = q(host, "recovery-keys")!.textContent!;
    expect(keys).toContain("Node storage key");
    expect(keys).toContain("aaaabbbbccccdddd");
    expect(keys).toContain("not created yet");
    expect(q(host, "recovery-scope")!.textContent).toMatch(/wallet/);
  });

  it("the file form uses password fields and shows the Rust refusal on a wrong kit", async () => {
    const wrong = "This recovery kit belongs to a different install. Its keys do not match the data on this computer, so nothing was changed.";
    const invoke = vi.fn((cmd: string) => {
      if (cmd === "recovery_kit_status") return status;
      throw new Error(wrong);
    });
    const { host } = await mount(<RecoveryKitPanel io={makeIo(invoke)} now={() => new Date()} toast={() => {}} />);
    await click(q(host, "kit-form-file"));
    expect(q(host, "kit-pass")!.getAttribute("type")).toBe("password");
    await click(q(host, "restore-form-file"));
    await type(q(host, "restore-pass"), "a long recovery passphrase");
    await click(q(host, "kit-restore"));
    expect(q(host, "recovery-error")!.textContent).toBe(wrong);
    expect(invoke).toHaveBeenCalledWith("recovery_kit_restore_file", { path: "/Users/me/kit.citrate-recovery", passphrase: "a long recovery passphrase", replace: false });
  });

  it("saving a phrase sheet toasts the path and asks to print then delete", async () => {
    const toast = vi.fn();
    const invoke = vi.fn((cmd: string) => (cmd === "recovery_kit_status" ? status : 2));
    const { host } = await mount(<RecoveryKitPanel io={makeIo(invoke)} now={() => new Date("2026-10-01T00:00:00Z")} toast={toast} />);
    await click(q(host, "kit-save"));
    expect(invoke).toHaveBeenCalledWith("recovery_kit_save_phrase", { path: "/Users/me/kit.citrate-recovery.txt" });
    expect(toast.mock.calls[0][0]).toMatch(/Print it, keep it offline, then delete the file/);
  });
});

describe("delete my local data panel", () => {
  it("needs a dry run, then the typed phrase, before the delete button works", async () => {
    const invoke = vi.fn((cmd: string) => {
      if (cmd === "local_data_plan") return plan;
      return { deleted: ["/d/ai.citrate.core/node"], failed: [], keychainDeleted: ["ai.citrate.core/node-storage-key"], keychainFailed: [] };
    });
    const { host } = await mount(<DeleteLocalDataPanel io={makeIo(invoke)} now={() => new Date()} toast={() => {}} />);
    expect(q(host, "delete-go")).toBeNull();
    await click(q(host, "delete-dry-run"));
    const shown = q(host, "delete-plan")!.textContent!;
    expect(shown).toContain("/d/ai.citrate.core/node");
    expect(shown).toContain("keep");
    expect(shown).toContain("node-storage-key");
    const go = q(host, "delete-go") as HTMLButtonElement;
    expect(go.disabled).toBe(true);
    await type(q(host, "delete-confirm"), "delete my local data");
    expect((q(host, "delete-go") as HTMLButtonElement).disabled).toBe(false);
    await click(q(host, "delete-go"));
    expect(invoke).toHaveBeenCalledWith("local_data_delete", { options: { includeWallet: false, keepModels: false }, confirm: "delete my local data", walletConfirm: null });
    expect(q(host, "delete-report")!.textContent).toMatch(/closes/);
  });

  it("including the wallet asks for the second phrase", async () => {
    const walletPlan = { ...plan, options: { includeWallet: true, keepModels: false }, walletConfirmPhrase: "delete my wallet" };
    const { host } = await mount(<DeleteLocalDataPanel io={makeIo(() => walletPlan)} now={() => new Date()} toast={() => {}} />);
    await click(q(host, "opt-include-wallet"));
    await click(q(host, "delete-dry-run"));
    await type(q(host, "delete-confirm"), "delete my local data");
    expect((q(host, "delete-go") as HTMLButtonElement).disabled).toBe(true);
    await type(q(host, "delete-wallet-confirm"), "delete my wallet");
    expect((q(host, "delete-go") as HTMLButtonElement).disabled).toBe(false);
  });
});

describe("privacy section (static)", () => {
  const html = renderToStaticMarkup(<PrivacySection io={makeIo(() => status)} now={() => new Date()} toast={() => {}} />);
  it("lists every offline matrix feature", () => {
    for (const f of matrix.features) expect(html).toContain(f.feature);
  });
  it("labels the budget values as pending owner sign-off", () => {
    expect(html).toContain("pending owner sign-off");
  });
  it("has no em-dashes or banned words", () => {
    expect(html).not.toContain("—");
    expect(html.toLowerCase()).not.toContain("dogfood");
    expect(html).not.toContain("HITL");
  });
});
