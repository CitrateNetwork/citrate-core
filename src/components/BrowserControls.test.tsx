// HUP-S5.1 + S5.6 — the main window's browser controls. Hidden while Hermes's browser is off (the
// default), so nothing changes for members. When it is on: decide on the action that is waiting,
// consent to an origin (a banking, email or health site needs a separate, explicit include),
// attach to your own Chrome only after ticking consent for this session, detach, Stop and resume.
import { describe, it, expect, afterEach, vi } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { BrowserControls } from "./BrowserControls";
import type { BrowserApi } from "../popout/browserApi";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let root: Root | null = null;
let host: HTMLDivElement | null = null;
afterEach(() => {
  act(() => root?.unmount());
  host?.remove();
  root = null;
  host = null;
});
const tick = () => act(async () => { await new Promise((r) => setTimeout(r, 0)); });
async function render(el: React.ReactElement) {
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
  await act(async () => { root?.render(el); });
  await tick();
  return host;
}
const q = (el: HTMLElement, id: string) => el.querySelector(`[data-testid="${id}"]`) as HTMLElement | null;

const ON = { enabled: true, chromium: { state: "system", path: "/c" }, mode: "managed", stopped: false, url: "https://a.example/", consentedOrigins: [], excludedCategories: [{ id: "banking", label: "Banking, payments and exchanges" }], consentNeeded: null, pendingAction: null };

function api(status: unknown, over: Partial<BrowserApi> = {}): BrowserApi & { calls: string[] } {
  const calls: string[] = [];
  const rec = (name: string) => vi.fn(async (...args: unknown[]) => { calls.push(`${name}(${args.map((a) => JSON.stringify(a)).join(",")})`); });
  return {
    calls,
    status: vi.fn(async () => status),
    frame: vi.fn(async () => null),
    stop: rec("stop") as BrowserApi["stop"],
    resume: rec("resume") as BrowserApi["resume"],
    attach: rec("attach") as BrowserApi["attach"],
    detach: rec("detach") as BrowserApi["detach"],
    origin: vi.fn(async (o: string, allow: boolean, inc: boolean) => { calls.push(`origin(${JSON.stringify(o)},${allow},${inc})`); return o; }) as BrowserApi["origin"],
    decide: rec("decide") as BrowserApi["decide"],
    ...over,
  };
}

const click = async (el: HTMLElement | null) => {
  expect(el).not.toBeNull();
  await act(async () => { el?.click(); });
  await tick();
};

describe("HUP-S5.1 browser controls", () => {
  it("render nothing while the browser is off (the default)", async () => {
    const el = await render(<BrowserControls api={api({ enabled: false })} onOpen={() => undefined} />);
    expect(q(el, "browser-controls")).toBeNull();
    expect(el.textContent).toBe("");
  });

  it("allow or deny the waiting action", async () => {
    const a = api({ ...ON, pendingAction: { id: "b7", tool: "browser_act", summary: 'Click [e2] button "Buy"', reason: "this session read untrusted content" } });
    const el = await render(<BrowserControls api={a} onOpen={() => undefined} />);
    expect(q(el, "browser-pending-summary")?.textContent).toContain('Click [e2] button "Buy"');
    await click(q(el, "browser-deny"));
    expect(a.calls).toContain('decide("b7",false)');
    await click(q(el, "browser-allow"));
    expect(a.calls).toContain('decide("b7",true)');
  });

  it("an ordinary origin can be allowed in one step", async () => {
    const a = api({ ...ON, mode: "attached", attachPort: 9222, consentNeeded: { origin: "https://docs.example.org", category: null } });
    const el = await render(<BrowserControls api={a} onOpen={() => undefined} />);
    await click(q(el, "browser-allow-origin"));
    expect(a.calls).toContain('origin("https://docs.example.org",true,false)');
  });

  it("a sensitive origin needs the explicit include box ticked first", async () => {
    const a = api({ ...ON, mode: "attached", attachPort: 9222, consentNeeded: { origin: "https://www.chase.com", category: "banking" } });
    const el = await render(<BrowserControls api={a} onOpen={() => undefined} />);
    expect(q(el, "browser-consent-needed")?.textContent).toContain("Banking, payments and exchanges");
    const allow = q(el, "browser-allow-origin") as HTMLButtonElement;
    expect(allow.disabled).toBe(true);
    await click(q(el, "browser-include-sensitive"));
    expect((q(el, "browser-allow-origin") as HTMLButtonElement).disabled).toBe(false);
    await click(q(el, "browser-allow-origin"));
    expect(a.calls).toContain('origin("https://www.chase.com",true,true)');
  });

  it("attach needs the consent box, then sends the port with consent", async () => {
    const a = api(ON);
    const el = await render(<BrowserControls api={a} onOpen={() => undefined} />);
    const attach = q(el, "browser-attach") as HTMLButtonElement;
    expect(attach.disabled).toBe(true);
    await click(q(el, "browser-attach-consent"));
    await click(q(el, "browser-attach"));
    expect(a.calls).toContain("attach(9222,true)");
  });

  it("an attached Chrome lists consented origins with revoke, and can detach", async () => {
    const a = api({ ...ON, mode: "attached", attachPort: 9333, consentedOrigins: ["https://docs.example.org"] });
    const el = await render(<BrowserControls api={a} onOpen={() => undefined} />);
    expect(q(el, "browser-attach")).toBeNull();
    await click(q(el, "browser-revoke-0"));
    expect(a.calls).toContain('origin("https://docs.example.org",false,false)');
    await click(q(el, "browser-detach"));
    expect(a.calls).toContain("detach()");
  });

  it("Stop when running, Resume when stopped, and errors are shown", async () => {
    let a = api(ON);
    let el = await render(<BrowserControls api={a} onOpen={() => undefined} />);
    await click(q(el, "browser-stop-main"));
    expect(a.calls).toContain("stop()");
    act(() => root?.unmount());

    a = api({ ...ON, stopped: true, mode: "off" }, { resume: vi.fn(async () => { throw new Error("hermes control returned 404: the browser is off"); }) });
    el = await render(<BrowserControls api={a} onOpen={() => undefined} />);
    await click(q(el, "browser-resume"));
    expect(q(el, "browser-error")?.textContent).toContain("the browser is off");
  });

  it("says when no Chromium is installed, and opens the pop-out", async () => {
    const onOpen = vi.fn();
    const el = await render(<BrowserControls api={api({ ...ON, mode: "off", chromium: { state: "not_installed", searched: ["/a"] } })} onOpen={onOpen} />);
    expect(el.textContent).toMatch(/No Chromium is installed/);
    await click(q(el, "browser-open"));
    expect(onOpen).toHaveBeenCalled();
  });
});

describe("HUP-S2.3 sign-in requests from the managed browser", () => {
  const REQ = { id: "signin-1-5", kind: "personal_sign", raiseOrigin: "https://app.example.org", topFrame: true, messageHex: "6869", createdMs: 1 };

  it("hands each waiting request to core once, and says a site is asking", async () => {
    const seen: string[] = [];
    const a = api({ ...ON, signInRequests: [REQ] });
    const el = await render(<BrowserControls api={a} onOpen={() => undefined} onSignInRequest={(id) => seen.push(id)} />);
    expect(seen).toEqual(["signin-1-5"]);
    expect(q(el, "browser-sign-in")?.textContent).toContain("https://app.example.org is asking to sign in with your wallet");
    await act(async () => { root?.render(<BrowserControls api={a} onOpen={() => undefined} onSignInRequest={(id) => seen.push(id)} />); });
    await tick();
    expect(seen).toEqual(["signin-1-5"]);
  });

  it("drops malformed requests and passes nothing it cannot name", async () => {
    const seen: string[] = [];
    const a = api({ ...ON, signInRequests: [{ ...REQ, id: "nope" }, { ...REQ, id: "signin-2-2", kind: "eth_sendTransaction" }] });
    const el = await render(<BrowserControls api={a} onOpen={() => undefined} onSignInRequest={(id) => seen.push(id)} />);
    expect(seen).toEqual([]);
    expect(q(el, "browser-sign-in")).toBeNull();
  });
});
