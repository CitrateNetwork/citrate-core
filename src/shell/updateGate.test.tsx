// #678 review: while an update is required the app is not rendered (nothing behind the gate can be
// focused or clicked), focus starts in and stays within the gate's dialog, app shortcuts do not
// fire, and external links open through core's open_external with the exact URL.
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";

const openExternal = vi.fn(async (_url: string) => {});
vi.mock("../bridge", () => ({ bridge: { mode: "tauri", shell: { openExternal: (u: string) => openExternal(u) } } }));

import { GateOrApp, MANUAL_DOWNLOAD_URL, blockShortcutsWhileGated, type AppUpdate } from "./UpdateBanner";
import { NetworkCompatGate } from "./NetworkCompatGate";
import { INITIAL_UPDATE_STATE, withVerdict, type UpdateState } from "./updater";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const gatedState = (over: Partial<UpdateState> = {}): UpdateState => ({
  ...withVerdict(INITIAL_UPDATE_STATE, { currentVersion: "0.5.3", minimumVersion: "0.5.4", updateRequired: true, reason: null, refused: null }),
  ...over,
});
const update = (state: UpdateState): AppUpdate => ({
  state,
  check: vi.fn(async () => {}),
  install: vi.fn(async () => {}),
  restart: vi.fn(async () => {}),
  dismiss: vi.fn(),
});

/** Stands in for the app: a wallet send button, a sign button and a text field. */
function FakeApp({ onSend }: { onSend: () => void }) {
  return (
    <div data-app="">
      <button onClick={onSend}>Send SALT</button>
      <button>Sign</button>
      <input aria-label="amount" />
    </div>
  );
}

let el: HTMLDivElement;
let root: Root;
beforeEach(() => {
  el = document.createElement("div");
  document.body.appendChild(el);
  root = createRoot(el);
  openExternal.mockClear();
});
afterEach(async () => {
  await act(async () => root.unmount());
  el.remove();
});

const tab = (shift = false) => {
  const t = (document.activeElement as HTMLElement) ?? document.body;
  t.dispatchEvent(new KeyboardEvent("keydown", { key: "Tab", shiftKey: shift, bubbles: true, cancelable: true }));
};

describe("while gated, nothing of the app exists", () => {
  it("no background control is in the document, so none is focusable or clickable", async () => {
    const onSend = vi.fn();
    await act(async () => root.render(<GateOrApp update={update(gatedState({ status: "ready", version: "0.5.4" }))}><FakeApp onSend={onSend} /></GateOrApp>));
    expect(el.querySelector("[data-app]")).toBeNull();
    expect([...el.querySelectorAll("button")].map((b) => b.textContent)).not.toContain("Send SALT");
    expect(el.querySelector('[role="dialog"][aria-modal="true"]')).not.toBeNull();
    // The update card lives inside the dialog, with its Restart.
    const dialog = el.querySelector('[role="dialog"]')!;
    expect([...dialog.querySelectorAll("button")].some((b) => b.textContent === "Restart now")).toBe(true);
    expect(onSend).not.toHaveBeenCalled();
  });

  it("not gated: the app renders as before", async () => {
    await act(async () => root.render(<GateOrApp update={update(INITIAL_UPDATE_STATE)}><FakeApp onSend={() => {}} /></GateOrApp>));
    expect(el.querySelector("[data-app]")).not.toBeNull();
    expect(el.querySelector('[role="dialog"]')).toBeNull();
  });

  it("App wraps its whole tree in GateOrApp", () => {
    const app = readFileSync(resolve(__dirname, "..", "App.tsx"), "utf8");
    const open = app.indexOf("<GateOrApp update={update}>");
    const close = app.indexOf("</GateOrApp>");
    expect(open).toBeGreaterThan(0);
    const inside = app.slice(open, close);
    for (const part of ["{body}", "<SignatureCeremony", "<WalletReviewModal", "<NetworkCompatGate", "<UpdateBanner"]) expect(inside, part).toContain(part);
  });
});

describe("focus starts in and stays within the dialog", () => {
  it("initial focus is inside, and Tab / Shift+Tab wrap inside", async () => {
    // Something outside the gate (as if left over) had focus.
    const outside = document.createElement("button");
    outside.textContent = "outside";
    document.body.appendChild(outside);
    outside.focus();
    await act(async () => root.render(<GateOrApp update={update(gatedState({ status: "error", error: "network down" }))}><FakeApp onSend={() => {}} /></GateOrApp>));
    const dialog = el.querySelector<HTMLElement>('[role="dialog"]')!;
    expect(dialog.contains(document.activeElement)).toBe(true);
    for (let i = 0; i < 12; i++) {
      tab(i % 3 === 0);
      expect(dialog.contains(document.activeElement), `step ${i}`).toBe(true);
    }
    // Focus that lands outside is pulled back in.
    await act(async () => outside.focus());
    expect(dialog.contains(document.activeElement)).toBe(true);
    outside.remove();
  });
});

describe("app shortcuts do not fire while gated", () => {
  it("a Cmd/Ctrl shortcut registered by the app never reaches it; copy still works", async () => {
    const appShortcut = vi.fn();
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key === "b") appShortcut();
    };
    window.addEventListener("keydown", onKey);
    document.addEventListener("keydown", onKey);
    await act(async () => root.render(<GateOrApp update={update(gatedState())}><FakeApp onSend={() => {}} /></GateOrApp>));
    const press = (init: KeyboardEventInit) => {
      const ev = new KeyboardEvent("keydown", { bubbles: true, cancelable: true, ...init });
      (document.activeElement ?? document.body).dispatchEvent(ev);
      return ev;
    };
    press({ key: "b", metaKey: true });
    press({ key: "b", ctrlKey: true });
    press({ key: "k", altKey: true });
    expect(appShortcut).not.toHaveBeenCalled();
    expect(press({ key: "c", metaKey: true }).defaultPrevented).toBe(false);
    // Lifted when the gate goes.
    await act(async () => root.render(<GateOrApp update={update(INITIAL_UPDATE_STATE)}><FakeApp onSend={() => {}} /></GateOrApp>));
    press({ key: "b", metaKey: true });
    expect(appShortcut).toHaveBeenCalledTimes(2);
    window.removeEventListener("keydown", onKey);
    document.removeEventListener("keydown", onKey);
  });

  it("blockShortcutsWhileGated unsubscribes cleanly", () => {
    const seen = vi.fn();
    window.addEventListener("keydown", seen);
    const off = blockShortcutsWhileGated();
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "b", metaKey: true }));
    expect(seen).not.toHaveBeenCalled();
    off();
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "b", metaKey: true }));
    expect(seen).toHaveBeenCalledTimes(1);
    window.removeEventListener("keydown", seen);
  });
});

describe("external links open through open_external with the exact URL", () => {
  it("Download manually", async () => {
    await act(async () => root.render(<GateOrApp update={update(gatedState())}><FakeApp onSend={() => {}} /></GateOrApp>));
    const link = [...el.querySelectorAll("a")].find((a) => a.textContent === "www.citrate.ai/download")!;
    expect(link).toBeTruthy();
    expect(link.getAttribute("target")).toBeNull();
    const ev = new MouseEvent("click", { bubbles: true, cancelable: true });
    await act(async () => link.dispatchEvent(ev));
    expect(ev.defaultPrevented).toBe(true);
    expect(openExternal).toHaveBeenCalledWith(MANUAL_DOWNLOAD_URL);
    expect(openExternal).toHaveBeenCalledWith("https://www.citrate.ai/download");
  });

  it("NetworkCompatGate's download link uses open_external too", () => {
    const src = readFileSync(resolve(__dirname, "NetworkCompatGate.tsx"), "utf8");
    expect(src).toContain('<ExternalLink url="https://citrate.ai/download" />');
    expect(src).not.toMatch(/target="_blank"/);
    expect(NetworkCompatGate).toBeTypeOf("function");
  });

  it("the tauri bridge's openExternal is the open_external command with { url }", () => {
    const src = readFileSync(resolve(__dirname, "..", "bridge", "tauri", "index.ts"), "utf8");
    expect(src).toMatch(/async openExternal\(url: string\): Promise<void> \{\s*await invoke\("open_external", \{ url \}\);/);
  });
});
