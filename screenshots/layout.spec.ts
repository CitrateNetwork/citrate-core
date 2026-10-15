import { test, expect, type Page } from "@playwright/test";
import { mkdirSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

// ---------------------------------------------------------------------------
// HGW (v0.5.1): the Hermes section owns its scroll. A real-layout check, in SIM MODE only
// (vite dev in Playwright's Chromium; no Tauri runtime, no node, no keyring; the packaged
// app is never launched).
//
// For the Dashboard ("Hermes" in the sidebar) and the Agent surface, at the default window
// (1100x720), a large window (1440x900) and a small one (900x600):
//   - the document and the Shell's surface container never scroll,
//   - the chat pane and its composer are fully on screen,
//   - each column scrolls on its own, and a wheel at the end of one column moves nothing else,
//   - the chat stays on the latest message while a simulated stream grows it, detaches only
//     when the member scrolls up, and "Jump to latest" brings it back.
//
// Run: cd screenshots && npm run layout  (PNGs land in screenshots/out/layout-*.png)
// ---------------------------------------------------------------------------

const OUT = fileURLToPath(new URL("out/", import.meta.url));
mkdirSync(OUT, { recursive: true });
const SHOT_PREFIX = process.env.LAYOUT_SHOT_PREFIX ?? "layout";

type Win = { width: number; height: number };
const WINDOWS: Win[] = [
  { width: 1100, height: 720 },
  { width: 1440, height: 900 },
  { width: 900, height: 600 },
];
const ROUTES = ["dashboard", "agent"] as const;
type Route = (typeof ROUTES)[number];
const RAIL: Record<Route, string> = { dashboard: "dashboard-rail", agent: "agent-rail" };

async function boot(page: Page) {
  await page.goto("/");
  await expect
    .poll(async () => page.evaluate(() => Boolean((window as any).__citrateStore)), { timeout: 30_000 })
    .toBe(true);
  await page.locator('[data-register="charter"]').first().waitFor({ state: "visible", timeout: 15_000 });
  await page.addStyleTag({ content: ".sim-proto-affordance{display:none !important}" });
  await page.evaluate(() => {
    const st = (window as any).__citrateStore;
    st.setState({ stage: "done", signedIn: true });
  });
}

async function go(page: Page, route: Route) {
  await page.evaluate((r) => {
    window.location.hash = `#/${r}`;
  }, route);
  await page.locator('[data-testid="agent-chat"]').first().waitFor({ state: "attached", timeout: 15_000 });
  await page.waitForTimeout(500);
}

/** A long, realistic transcript so the message list overflows at every window size. */
async function seedChat(page: Page, n = 40) {
  await page.evaluate((count) => {
    const st = (window as any).__citrateStore;
    const msgs = Array.from({ length: count }, (_, i) => ({
      id: "seed-" + i,
      who: i % 2 ? "Agent" : "You",
      text: i % 2 ? "Here is a longer answer with a few sentences in it. ".repeat(4 + (i % 3)) : "Question number " + i,
      chips: [],
      streaming: false,
    }));
    st.setState({ chatMsgs: msgs, chatStatus: "ready" });
  }, n);
  await page.waitForTimeout(200);
}

type Geo = {
  docScroll: number;
  docClient: number;
  shellScrollTop: number;
  shellScroll: number;
  shellClient: number;
  innerHeight: number;
  paneTop: number;
  paneBottom: number;
  composerBottom: number;
  composerVisible: boolean;
  listTop: number;
  listScroll: number;
  listClient: number;
  listDist: number;
};

async function geo(page: Page): Promise<Geo> {
  return page.evaluate(() => {
    const q = (s: string) => document.querySelector(s) as HTMLElement | null;
    const shell = (q('[data-testid="surface-scroller"]') ?? q("main > div:last-child")) as HTMLElement;
    const pane = q('[data-testid="agent-chat"]') as HTMLElement;
    const composer = q('[data-testid="chat-composer"]') as HTMLElement;
    const list = q('[data-testid="chat-messages"]') as HTMLElement;
    const se = document.scrollingElement as HTMLElement;
    const pr = pane.getBoundingClientRect();
    const cr = composer.getBoundingClientRect();
    return {
      docScroll: se.scrollHeight,
      docClient: se.clientHeight,
      shellScrollTop: shell.scrollTop,
      shellScroll: shell.scrollHeight,
      shellClient: shell.clientHeight,
      innerHeight: window.innerHeight,
      paneTop: pr.top,
      paneBottom: pr.bottom,
      composerBottom: cr.bottom,
      composerVisible: cr.height > 0,
      listTop: list.getBoundingClientRect().top,
      listScroll: list.scrollHeight,
      listClient: list.clientHeight,
      listDist: list.scrollHeight - list.scrollTop - list.clientHeight,
    };
  });
}

async function scrollTopOf(page: Page, testid: string): Promise<number> {
  return page.evaluate((id) => (document.querySelector(`[data-testid="${id}"]`) as HTMLElement).scrollTop, testid);
}

async function wheelOver(page: Page, testid: string, dy: number) {
  const box = await page.locator(`[data-testid="${testid}"]`).first().boundingBox();
  if (!box) throw new Error(`${testid} is not on screen`);
  await page.mouse.move(box.x + box.width / 2, box.y + Math.min(box.height / 2, 120));
  await page.mouse.wheel(0, dy);
  await page.waitForTimeout(250);
}

/** Make a column taller than its box (sim data is sparse), so its own scroll is exercised. */
async function overfill(page: Page, testid: string) {
  await page.evaluate((id) => {
    const el = document.querySelector(`[data-testid="${id}"]`) as HTMLElement;
    const filler = document.createElement("div");
    filler.setAttribute("data-layout-filler", "");
    filler.style.height = "1600px";
    filler.style.flexShrink = "0";
    el.appendChild(filler);
  }, testid);
  await page.waitForTimeout(100);
}

/** Simulate a streamed reply: 32 ms coalesced deltas through the store, as sendChat flushes. */
async function stream(page: Page, ticks: number, opts: { start?: boolean } = {}): Promise<number[]> {
  return page.evaluate(
    async ({ ticks, start }) => {
      const st = (window as any).__citrateStore;
      const list = document.querySelector('[data-testid="chat-messages"]') as HTMLElement;
      const frame = () => new Promise((r) => requestAnimationFrame(() => r(null)));
      if (start) {
        st.setState({
          chatStatus: "thinking",
          chatMsgs: st.state.chatMsgs.concat([{ id: "stream", who: "Agent", text: "", chips: [], streaming: true }]),
        });
      }
      const dists: number[] = [];
      for (let i = 0; i < ticks; i++) {
        const chunk = i % 9 === 8 ? "\n\n```\ncode line " + i + "\nmore code\n```\n\n" : "streamed words for tick " + i + ". ";
        st.setState({
          chatMsgs: st.state.chatMsgs.map((m: any) => (m.id === "stream" ? { ...m, text: m.text + chunk } : m)),
        });
        await frame();
        await frame();
        dists.push(list.scrollHeight - list.scrollTop - list.clientHeight);
        await new Promise((r) => setTimeout(r, 32));
      }
      return dists;
    },
    { ticks, start: Boolean(opts.start) },
  );
}

async function endStream(page: Page) {
  await page.evaluate(() => {
    const st = (window as any).__citrateStore;
    st.setState({
      chatStatus: "ready",
      chatMsgs: st.state.chatMsgs.map((m: any) => (m.id === "stream" ? { ...m, streaming: false } : m)),
    });
  });
}

function shot(win: Win, route: Route, what: string) {
  return join(OUT, `${SHOT_PREFIX}-${route}-${win.width}x${win.height}-${what}.png`);
}

for (const win of WINDOWS) {
  test.describe(`${win.width}x${win.height}`, () => {
    test.use({ viewport: win, deviceScaleFactor: 1 });

    for (const route of ROUTES) {
      test(`${route}: the section is the window and every column scrolls on its own`, async ({ page }) => {
        await boot(page);
        await go(page, route);
        await seedChat(page);
        await page.screenshot({ path: shot(win, route, "1-initial") });

        // 1. No document scroll, no Shell scroll; the chat and its composer are on screen.
        let g = await geo(page);
        expect.soft(g.docScroll).toBeLessThanOrEqual(g.docClient);
        expect.soft(g.shellScroll).toBeLessThanOrEqual(g.shellClient + 1);
        expect.soft(g.composerVisible).toBe(true);
        expect.soft(g.composerBottom).toBeLessThanOrEqual(g.innerHeight);
        expect.soft(g.paneBottom).toBeLessThanOrEqual(g.innerHeight);
        // The transcript overflows and is pinned to its end.
        expect.soft(g.listScroll).toBeGreaterThan(g.listClient);
        expect.soft(g.listDist).toBeLessThan(2);

        // 2. A wheel at the end of the chat moves nothing else (no scroll chaining).
        const pane0 = g.paneTop;
        await wheelOver(page, "chat-messages", 600);
        g = await geo(page);
        expect.soft(g.shellScrollTop).toBe(0);
        expect.soft(g.paneTop).toBe(pane0);

        // 3. The chat scrolls on its own: the side column stays put.
        const narrow = await page.locator('[data-testid="section-switch"]').count();
        const rail = RAIL[route];
        const railTop0 = narrow ? 0 : await scrollTopOf(page, rail);
        await wheelOver(page, "chat-messages", -300);
        expect.soft(await scrollTopOf(page, "chat-messages")).toBeLessThan(g.listScroll - g.listClient - 100);
        if (!narrow) expect.soft(await scrollTopOf(page, rail)).toBe(railTop0);
        g = await geo(page);
        expect.soft(g.shellScrollTop).toBe(0);
        expect.soft(g.paneTop).toBe(pane0);
        await page.screenshot({ path: shot(win, route, "2-chat-scrolled-up") });

        // 4. The side column scrolls on its own (on a narrow window, behind its tab).
        if (narrow) {
          await page.locator('[data-testid="section-switch"] [role="tab"]').nth(1).click();
          await page.waitForTimeout(150);
          await expect(page.locator(`[data-testid="${rail}"]`)).toBeVisible();
          await expect(page.locator('[data-testid="agent-chat"]')).toBeHidden();
        }
        await overfill(page, rail);
        const chatTop1 = await scrollTopOf(page, "chat-messages");
        await wheelOver(page, rail, 400);
        expect.soft(await scrollTopOf(page, rail)).toBeGreaterThan(0);
        expect.soft(await scrollTopOf(page, "chat-messages")).toBe(chatTop1);
        await page.screenshot({ path: shot(win, route, "3-side-column-scrolled") });
        // ...and a wheel at its end chains nowhere.
        await page.evaluate((id) => {
          const el = document.querySelector(`[data-testid="${id}"]`) as HTMLElement;
          el.scrollTop = el.scrollHeight;
        }, rail);
        await wheelOver(page, rail, 600);
        const shellTop = await page.evaluate(() => (document.querySelector('[data-testid="surface-scroller"]') as HTMLElement).scrollTop);
        expect.soft(shellTop).toBe(0);
        if (narrow) {
          await page.locator('[data-testid="section-switch"] [role="tab"]').nth(0).click();
          await page.waitForTimeout(150);
          await expect(page.locator('[data-testid="agent-chat"]')).toBeVisible();
        }
        expect.soft((await geo(page)).paneTop).toBe(pane0);
      });

      test(`${route}: the chat stays on the latest message while a reply streams`, async ({ page }) => {
        await boot(page);
        await go(page, route);
        await seedChat(page);
        const pane0 = (await geo(page)).paneTop;

        // Pinned through a whole stream (plain words and code blocks), with no input.
        const dists = await stream(page, 60, { start: true });
        expect.soft(Math.max(...dists)).toBeLessThan(2);
        expect.soft((await geo(page)).paneTop).toBe(pane0);
        await page.screenshot({ path: shot(win, route, "4-streaming-pinned") });

        // The member scrolls up to read: the list stays where they put it.
        await wheelOver(page, "chat-messages", -500);
        const held = await scrollTopOf(page, "chat-messages");
        await stream(page, 15);
        expect.soft(Math.abs((await scrollTopOf(page, "chat-messages")) - held)).toBeLessThanOrEqual(1);
        const jump = page.locator('[data-testid="chat-jump-latest"]');
        await expect(jump).toBeVisible();
        expect.soft((await geo(page)).paneTop).toBe(pane0);
        await page.screenshot({ path: shot(win, route, "5-reading-back-jump-to-latest") });

        // "Jump to latest" re-pins, and the stream is followed again.
        await jump.click();
        await page.waitForTimeout(600);
        const after = await stream(page, 15);
        expect.soft(after[after.length - 1]).toBeLessThan(2);
        await expect(jump).toBeHidden();
        await endStream(page);

        // A new turn from anywhere (here: a workflow run) re-arms the follow.
        await wheelOver(page, "chat-messages", -500);
        await expect(jump).toBeVisible();
        await page.evaluate(() => {
          const st = (window as any).__citrateStore;
          st.setState({
            chatStatus: "thinking",
            chatMsgs: st.state.chatMsgs.concat([{ id: "wf", who: "Workflow", text: "Running the workflow.", chips: [], streaming: true }]),
          });
        });
        await page.waitForTimeout(300);
        expect.soft((await geo(page)).listDist).toBeLessThan(2);
        await expect(jump).toBeHidden();
      });
    }
  });
}

test.describe("1100x720 other surfaces and deep links", () => {
  test.use({ viewport: { width: 1100, height: 720 }, deviceScaleFactor: 1 });

  test("every other surface keeps its page scroll", async ({ page }) => {
    await boot(page);
    for (const route of ["wallet", "node", "settings"]) {
      await page.evaluate((r) => {
        window.location.hash = `#/${r}`;
      }, route);
      await page.waitForTimeout(400);
      const overflowY = await page.evaluate(() => getComputedStyle(document.querySelector('[data-testid="surface-scroller"]') as HTMLElement).overflowY);
      expect.soft(overflowY, route).toBe("auto");
    }
  });

  test("the chat's 'Open toolchain settings' deep link lands on the toolchain step", async ({ page }) => {
    await boot(page);
    await go(page, "agent");
    await page.evaluate(() => (window as any).__citrateStore.setState({ agentTabRequest: "contracts" }));
    const anchor = page.locator("#contract-toolchain");
    await anchor.waitFor({ state: "attached" });
    await page.waitForTimeout(400);
    const r = await anchor.evaluate((el) => {
      const b = el.getBoundingClientRect();
      const col = document.querySelector('[data-testid="agent-contracts-scroll"]') as HTMLElement;
      const c = col.getBoundingClientRect();
      return { top: b.top, colTop: c.top, colBottom: c.bottom, inCol: col.contains(el), shellTop: (document.querySelector('[data-testid="surface-scroller"]') as HTMLElement).scrollTop };
    });
    expect.soft(r.inCol).toBe(true);
    expect.soft(r.top).toBeGreaterThanOrEqual(r.colTop - 1);
    expect.soft(r.top).toBeLessThan(r.colBottom);
    expect.soft(r.shellTop).toBe(0);
    await page.screenshot({ path: join(OUT, `${SHOT_PREFIX}-agent-1100x720-6-toolchain-deep-link.png`) });
  });
});
