// HUP-S10.4 — the passphrase panel for encrypted journal export/import.
import { describe, it, expect, vi } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { renderToStaticMarkup } from "react-dom/server";
import type { JournalPage } from "../shell/state";
import { buildBundle } from "./bundle";
import type { JournalIo } from "./encryptedExport";
import { JournalVaultPanel } from "./JournalVaultPanel";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const PASS = "a long journal passphrase";
const NOW = new Date("2026-10-01T04:05:06Z");
const pages: JournalPage[] = [{ id: "d-2026-10-01", title: "2026-10-01", kind: "daily", pinned: false, blocks: ["hello"] }];

function makeIo(invoke: JournalIo["invoke"]): JournalIo {
  return {
    mode: "tauri",
    pickSavePath: async () => "/Users/me/out",
    pickOpenPath: async () => "/Users/me/in.citrate-journal",
    invoke,
  };
}

async function mount(el: React.ReactElement): Promise<{ host: HTMLDivElement; root: Root }> {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(el);
  });
  return { host, root };
}
const q = <T extends Element = HTMLElement>(host: HTMLElement, id: string) => host.querySelector(`[data-testid="${id}"]`) as T | null;
async function click(el: Element | null) {
  expect(el).toBeTruthy();
  await act(async () => {
    (el as HTMLElement).click();
  });
  await act(async () => {});
}
async function type(el: Element | null, value: string) {
  expect(el).toBeTruthy();
  const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")?.set;
  await act(async () => {
    setter?.call(el, value);
    el!.dispatchEvent(new Event("input", { bubbles: true }));
  });
}

describe("JournalVaultPanel — export", () => {
  it("renders password fields (never plain text) and says the file is encrypted with a passphrase only the member knows", () => {
    const html = renderToStaticMarkup(
      <JournalVaultPanel kind="export" pages={pages} io={async () => makeIo(vi.fn())} now={() => NOW} onImported={() => {}} onDone={() => {}} onClose={() => {}} />,
    );
    expect(html).toContain('type="password"');
    expect(html).not.toContain('type="text"');
    expect(html.toLowerCase()).toContain("cannot be recovered");
  });

  it("shows a mismatch inline and never calls the sealer", async () => {
    const invoke = vi.fn();
    const { host, root } = await mount(
      <JournalVaultPanel kind="export" pages={pages} io={async () => makeIo(invoke)} now={() => NOW} onImported={() => {}} onDone={() => {}} onClose={() => {}} />,
    );
    await type(q(host, "jv-pass"), PASS);
    await type(q(host, "jv-confirm"), PASS + "!");
    await click(q(host, "jv-go"));
    expect(q(host, "jv-error")?.textContent).toMatch(/do not match/);
    expect(invoke).not.toHaveBeenCalled();
    root.unmount();
  });

  it("seals through Rust, reports where it saved, and closes", async () => {
    const invoke = vi.fn(async () => 99) as unknown as JournalIo["invoke"];
    const onDone = vi.fn();
    const onClose = vi.fn();
    const { host, root } = await mount(
      <JournalVaultPanel kind="export" pages={pages} io={async () => makeIo(invoke)} now={() => NOW} onImported={() => {}} onDone={onDone} onClose={onClose} />,
    );
    await type(q(host, "jv-pass"), PASS);
    await type(q(host, "jv-confirm"), PASS);
    await click(q(host, "jv-go"));
    expect(invoke).toHaveBeenCalledWith("journal_export_encrypted", { path: "/Users/me/out.citrate-journal", passphrase: PASS, bundle: buildBundle(pages, NOW.toISOString()) });
    expect(onDone).toHaveBeenCalledWith(expect.stringContaining("/Users/me/out.citrate-journal"));
    expect(onClose).toHaveBeenCalled();
    root.unmount();
  });
});

describe("JournalVaultPanel — import", () => {
  it("shows a wrong passphrase inline and keeps the journal untouched", async () => {
    const invoke = vi.fn(async () => {
      throw new Error("That passphrase does not open this file, or the file was changed or damaged.");
    }) as unknown as JournalIo["invoke"];
    const onImported = vi.fn();
    const { host, root } = await mount(
      <JournalVaultPanel kind="import" pages={pages} io={async () => makeIo(invoke)} now={() => NOW} onImported={onImported} onDone={() => {}} onClose={() => {}} />,
    );
    expect(q(host, "jv-confirm")).toBeNull();
    await type(q(host, "jv-pass"), "wrong passphrase");
    await click(q(host, "jv-go"));
    expect(q(host, "jv-error")?.textContent).toMatch(/does not open this file/);
    expect(onImported).not.toHaveBeenCalled();
    root.unmount();
  });

  it("merges a good file and reports the counts", async () => {
    const incoming: JournalPage[] = [{ id: "p-x", title: "X", kind: "page", pinned: false, blocks: ["x"] }];
    const invoke = vi.fn(async () => buildBundle(incoming, NOW.toISOString())) as unknown as JournalIo["invoke"];
    const onImported = vi.fn();
    const onDone = vi.fn();
    const { host, root } = await mount(
      <JournalVaultPanel kind="import" pages={pages} io={async () => makeIo(invoke)} now={() => NOW} onImported={onImported} onDone={onDone} onClose={() => {}} />,
    );
    await type(q(host, "jv-pass"), PASS);
    await click(q(host, "jv-go"));
    expect(onImported).toHaveBeenCalledWith([...pages, incoming[0]]);
    expect(onDone).toHaveBeenCalledWith("Imported 1 new page, 0 already here, 0 kept as separate copies.");
    root.unmount();
  });
});
