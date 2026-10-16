// HUP-S5.4 — the Code and diff view: it starts where the main window points, lists the session's
// changes, shows a line diff with honest wording for what it cannot show, and is accessible
// (landmark, heading, pressed state, added and removed said in text, alerts for refusals).
import { describe, expect, it, vi, afterEach } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { DiffViewer } from "./DiffViewer";
import type { DiffClient, DiffOpArgs, DiffOpName, DiffOpResults } from "./diffChannel";
import type { StepDiff } from "./diffModel";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const LIST = {
  session: "s3-ab",
  enabled: true,
  steps: [
    { seq: 3, status: "committed", paths: ["src/app.ts"], root: "/w" },
    { seq: 1, status: "undone", paths: ["notes.md"], root: "/w" },
  ],
  note: null,
};

const D3: StepDiff = {
  ok: true,
  session: "s3-ab",
  seq: 3,
  status: "committed",
  files: [
    { path: "src/app.ts", before: { kind: "text", text: "const a = 1;\nconst b = 2;\n" }, after: { kind: "text", text: "const a = 1;\nconst b = 3;\n" } },
    { path: "logo.png", before: { kind: "absent" }, after: { kind: "binary", size: 2048 } },
  ],
  kind: null,
  reason: null,
};
const D1: StepDiff = {
  ok: true,
  session: "s3-ab",
  seq: 1,
  status: "undone",
  files: [{ path: "notes.md", before: { kind: "text", text: "hi\n" }, after: { kind: "unavailable", reason: "this step was undone, so its result is no longer on disk" } }],
  kind: null,
  reason: null,
};

function client(over: Partial<{ [K in DiffOpName]: (a: DiffOpArgs[K]) => Promise<DiffOpResults[K]> }> = {}) {
  const impl = {
    initial: async () => ({ session: "s3-ab", seq: null }),
    steps: async () => LIST,
    diff: async (a: DiffOpArgs["diff"]) => (a.seq === 3 ? D3 : D1),
    ...over,
  } as Record<string, (a: unknown) => Promise<unknown>>;
  const call = vi.fn((op: string, args: unknown) => impl[op](args));
  return { client: { call, close: vi.fn() } as unknown as DiffClient, call };
}

let root: Root | null = null;
let host: HTMLDivElement | null = null;
afterEach(() => {
  act(() => root?.unmount());
  host?.remove();
  root = null;
  host = null;
});
async function render(el: React.ReactElement) {
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
  await act(async () => {
    root?.render(el);
  });
  for (let i = 0; i < 4; i++) {
    await act(async () => {
      await new Promise((r) => setTimeout(r, 0));
    });
  }
  return host;
}
const q = (el: HTMLElement, id: string) => el.querySelector(`[data-testid="${id}"]`) as HTMLElement | null;
const all = (el: HTMLElement, id: string) => Array.from(el.querySelectorAll(`[data-testid="${id}"]`)) as HTMLElement[];

describe("HUP-S5.4 DiffViewer", () => {
  it("opens the newest change of the session and shows its line diff", async () => {
    const c = client();
    const el = await render(<DiffViewer client={c.client} focus={null} />);
    const steps = all(el, "diff-step");
    expect(steps.map((s) => s.textContent)).toEqual(["Change 3: src/app.ts", "Change 1: notes.md (undone)"]);
    expect(steps[0].getAttribute("aria-pressed")).toBe("true");
    expect(c.call).toHaveBeenCalledWith("diff", { session: "s3-ab", seq: 3 });
    const files = all(el, "diff-file");
    expect(files).toHaveLength(2);
    expect(q(files[0], "diff-stats")?.textContent).toBe("1 added, 1 removed");
    expect(all(files[0], "diff-line-add")[0].textContent).toContain("const b = 3;");
    expect(all(files[0], "diff-line-del")[0].textContent).toContain("const b = 2;");
    expect(q(files[1], "diff-after-note")?.textContent).toBe("After: Binary content (2 KB), not shown.");
    expect(q(files[1], "diff-before-note")?.textContent).toMatch(/did not exist/);
  });

  it("is accessible: a main landmark with a heading, named regions, and changes said in text", async () => {
    const el = await render(<DiffViewer client={client().client} focus={null} />);
    const main = el.querySelector("main");
    expect(main?.getAttribute("aria-labelledby")).toBeTruthy();
    expect(document.getElementById(main?.getAttribute("aria-labelledby") ?? "")?.tagName).toBe("H1");
    const region = all(el, "diff-file")[0];
    expect(region.tagName).toBe("SECTION");
    const named = region.getAttribute("aria-labelledby") ?? "";
    expect(document.getElementById(named)?.textContent).toBe("src/app.ts");
    expect(el.querySelector("nav")?.getAttribute("aria-label")).toBe("Agent file changes");
    const added = all(el, "diff-line-add")[0];
    expect(added.textContent).toContain("added");
    expect(added.querySelector('[aria-hidden="true"]')?.textContent).toBe("+");
    expect(el.querySelector("caption")?.textContent).toBe("Line changes in src/app.ts");
    for (const b of el.querySelectorAll("button")) expect(b.getAttribute("type")).toBe("button");
    expect(el.textContent ?? "").not.toContain("—");
  });

  it("picking another change loads it, and an undone change says why its result is not shown", async () => {
    const el = await render(<DiffViewer client={client().client} focus={null} />);
    await act(async () => {
      all(el, "diff-step")[1].click();
    });
    await act(async () => {
      await new Promise((r) => setTimeout(r, 0));
    });
    expect(all(el, "diff-step")[1].getAttribute("aria-pressed")).toBe("true");
    expect(q(el, "diff-undone")?.textContent).toMatch(/was undone/);
    expect(q(el, "diff-after-note")?.textContent).toMatch(/no longer on disk/);
  });

  it("starts on the change the main window named, and follows a later focus", async () => {
    const c = client({ initial: async () => ({ session: "s3-ab", seq: 1 }) });
    const el = await render(<DiffViewer client={c.client} focus={null} />);
    expect(c.call).toHaveBeenCalledWith("diff", { session: "s3-ab", seq: 1 });
    expect(all(el, "diff-step")[1].getAttribute("aria-pressed")).toBe("true");
    await act(async () => {
      root?.render(<DiffViewer client={c.client} focus={{ session: "s3-ab", seq: 3 }} />);
    });
    for (let i = 0; i < 3; i++)
      await act(async () => {
        await new Promise((r) => setTimeout(r, 0));
      });
    expect(c.call).toHaveBeenCalledWith("diff", { session: "s3-ab", seq: 3 });
  });

  it("says plainly when there is nothing to show, undo is off, or the sidecar refused", async () => {
    let el = await render(<DiffViewer client={client({ initial: async () => null }).client} focus={null} />);
    expect(q(el, "diff-empty")?.textContent).toMatch(/No agent file changes yet/);
    act(() => root?.unmount());
    el = await render(<DiffViewer client={client({ steps: async () => ({ session: "s3-ab", enabled: false, steps: [], note: "undo checkpoints are not enabled in this agent sidecar" }) }).client} focus={null} />);
    expect(q(el, "diff-disabled")?.textContent).toMatch(/not enabled/);
    act(() => root?.unmount());
    el = await render(
      <DiffViewer client={client({ diff: async () => ({ ...D3, ok: false, files: [], status: "", kind: "pruned", reason: "step 3 of s3-ab was pruned" }) }).client} focus={null} />,
    );
    const refused = q(el, "diff-refused");
    expect(refused?.getAttribute("role")).toBe("alert");
    expect(refused?.textContent).toMatch(/pruned/);
    act(() => root?.unmount());
    el = await render(<DiffViewer client={client({ steps: async () => { throw new Error("hermes is not running"); } }).client} focus={null} />);
    expect(q(el, "diff-error")?.getAttribute("role")).toBe("alert");
    expect(q(el, "diff-error")?.textContent).toMatch(/hermes is not running/);
  });

  it("asks nothing that could change a file", async () => {
    const c = client();
    await render(<DiffViewer client={c.client} focus={null} />);
    const ops = new Set(c.call.mock.calls.map((x) => x[0]));
    expect([...ops].every((o) => ["initial", "steps", "diff"].includes(o as string))).toBe(true);
  });
});
