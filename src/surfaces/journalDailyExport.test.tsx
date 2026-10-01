// HUP-S10.4 — the Journal surface offers the daily entry, the local-records Hermes
// summary, and the encrypted export/import, and its caption stays honest.
import { describe, it, expect, vi } from "vitest";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { renderToStaticMarkup } from "react-dom/server";
import { Journal } from "./Journal";
import { freshState, type AppState } from "../shell/state";
import type { Store } from "../shell/store";
import { HERMES_SUMMARY_HEADER } from "../journal/dailyEntry";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const noopStore = {} as unknown as Store;
const today = () => new Date().toISOString().slice(0, 10);

function state(over: Partial<AppState> = {}): AppState {
  const s = freshState("p1");
  s.route = "journal";
  return Object.assign(s, over);
}

describe("Journal surface — HUP-S10.4", () => {
  it("offers a Today button for the one-per-day entry", () => {
    const html = renderToStaticMarkup(<Journal store={noopStore} s={state()} />);
    expect(html).toContain('data-testid="j-today"');
  });

  it("offers the Hermes summary on today's entry only", () => {
    const s = state();
    s.jSel = "d-" + today();
    expect(renderToStaticMarkup(<Journal store={noopStore} s={s} />)).toContain('data-testid="j-summary"');
    s.jPages = s.jPages.concat([{ id: "p-x", title: "X", kind: "page", pinned: false, blocks: [] }]);
    s.jSel = "p-x";
    expect(renderToStaticMarkup(<Journal store={noopStore} s={s} />)).not.toContain('data-testid="j-summary"');
  });

  it("lists encrypted export and import in the export menu", () => {
    const html = renderToStaticMarkup(<Journal store={noopStore} s={state({ jExportOpen: true })} />);
    expect(html).toContain("Encrypted file");
    expect(html).toContain(".citrate-journal");
    expect(html).toContain("Import encrypted file");
  });

  it("caption says what is true: local storage, and the encrypted export is the way a copy leaves the device", () => {
    const html = renderToStaticMarkup(<Journal store={noopStore} s={state()} />).toLowerCase();
    expect(html).toContain("stored locally on this device");
    expect(html).toContain("encrypted export");
    expect(html).not.toContain("encrypted at rest in your data dir");
  });

  it("Today creates the entry once and the summary is written from local records", async () => {
    let s = state({ jPages: [], jSel: null, activity: [] });
    const store = {
      get state() {
        return s;
      },
      setState: vi.fn((u: Partial<AppState> | ((st: AppState) => Partial<AppState>)) => {
        s = { ...s, ...(typeof u === "function" ? u(s) : u) };
      }),
      save: vi.fn(),
      toast: vi.fn(),
    } as unknown as Store;
    const host = document.createElement("div");
    document.body.appendChild(host);
    const root = createRoot(host);
    const render = async () => {
      await act(async () => {
        root.render(<Journal store={store} s={s} />);
      });
    };
    await render();
    await act(async () => {
      (host.querySelector('[data-testid="j-today"]') as HTMLElement).click();
    });
    await render();
    await act(async () => {
      (host.querySelector('[data-testid="j-today"]') as HTMLElement).click();
    });
    expect(s.jPages.filter((p) => p.id === "d-" + today())).toHaveLength(1);
    expect(s.jSel).toBe("d-" + today());
    await render();
    await act(async () => {
      (host.querySelector('[data-testid="j-summary"]') as HTMLElement).click();
    });
    const entry = s.jPages.find((p) => p.id === "d-" + today());
    expect(entry?.blocks).toContain(HERMES_SUMMARY_HEADER);
    expect(entry?.blocks.join("\n")).toContain("No Hermes activity is recorded on this device today.");
    root.unmount();
  });
});
