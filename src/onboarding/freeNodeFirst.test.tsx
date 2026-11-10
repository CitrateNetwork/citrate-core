// Owner decision 2026-10-27 (option a): onboarding offers the FREE path first, so nobody thinks
// paying is required to run a node. The membership price appears ONLY on the member checkout
// (MembershipCheckout), never on the choice, never on any other onboarding step, never in the shell.
//
// What this pins:
//   1. S3 opens on a choice: "Run a node for free" (primary) before "Become a member (optional)".
//   2. The free option works without payment: it continues onboarding (S3 → S4 → S6 node
//      ignition) with no checkout, no wallet link and no price read.
//   3. Landing on S3 with a linked wallet never opens checkout on its own; only choosing
//      membership does.
//   4. Tripwire: no price figure ("$" + digits, or 48) renders on any onboarding step outside the
//      member checkout, and no source outside MembershipCheckout carries one.
import { describe, it, expect, vi, beforeEach } from "vitest";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { renderToStaticMarkup } from "react-dom/server";
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";
// S0 may show a genesis-checked network name (#349) read through wagmi; a static render has no
// WagmiProvider, so that one hook reads "no block yet". Everything else in wagmi stays real.
vi.mock("wagmi", async (orig) => ({
  ...(await orig<typeof import("wagmi")>()),
  useBlock: () => ({ data: undefined }),
}));

import { Onboarding, S3, S4 } from "./Onboarding";
import { freshState, type AppState } from "../shell/state";
import { store as realStore, type Store } from "../shell/store";
import { bridge } from "../bridge";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

/**
 * A price figure: "$48", "$ 48", "$0", "$48.00"; "48 USD", "USD 48"; or a bare number with a
 * billing period ("48 / year", "48 a year", "48 per year"). A bare 48 alone (a style or size
 * value) is not a price.
 */
const PRICE = /\$\s?\d|\bUSD\s?\d|\d\s?USD\b|\b\d[\d.,]*\s*(?:\/\s*|a |per )(?:year|yr|month|mo)\b/i;

function textOf(html: string): string {
  return html.replace(/<[^>]*>/g, " ");
}

function at(stage: AppState["stage"], patch: Partial<AppState> = {}): AppState {
  const s = freshState("p1");
  s.stage = stage;
  s.s3 = "idle";
  return Object.assign(s, patch);
}

// Every store method a static render might touch returns undefined (falsy), never a price.
const inertStore = new Proxy({}, { get: () => () => undefined }) as unknown as Store;

function spyStore(linked: boolean) {
  return {
    onS3Free: vi.fn(),
    onS3Pay: vi.fn(),
    endS3Check: vi.fn(),
    linkWallet: vi.fn(),
    openExternal: vi.fn(),
    walletIsLinked: vi.fn().mockReturnValue(linked),
    setState: vi.fn(),
    save: vi.fn(),
    recheckMembership: vi.fn(),
    submitEnterpriseLead: vi.fn(),
    // C1-P (#379): the checkout reads the price from core-membership while it is on screen.
    watchMembershipPrice: vi.fn(() => () => {}),
  };
}

async function mount(node: React.ReactElement) {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(node);
  });
  return { host, root };
}

function button(host: HTMLElement, re: RegExp): HTMLButtonElement | undefined {
  return Array.from(host.querySelectorAll("button")).find((b) => re.test(b.textContent ?? ""));
}

describe("S3 — the free node is offered first", () => {
  it("offers 'Run a node for free' before any paid option, and shows no price", () => {
    const html = renderToStaticMarkup(<S3 store={inertStore} s={at("s3")} />);
    const text = textOf(html);
    const free = text.indexOf("Run a node for free");
    const member = text.indexOf("Become a member");
    expect(free).toBeGreaterThan(-1);
    expect(member).toBeGreaterThan(-1);
    expect(free).toBeLessThan(member);
    // No checkout control and no price on the choice.
    expect(text).not.toContain("Check out in your browser");
    expect(text).not.toMatch(PRICE);
  });

  it("the free option continues without payment: no checkout, no wallet link, no external page", async () => {
    const st = spyStore(true);
    const { host, root } = await mount(<S3 store={st as unknown as Store} s={at("s3")} />);
    const free = button(host, /Run a node for free/);
    expect(free).toBeTruthy();
    await act(async () => free!.click());
    expect(st.onS3Free).toHaveBeenCalledTimes(1);
    expect(st.onS3Pay).not.toHaveBeenCalled();
    expect(st.linkWallet).not.toHaveBeenCalled();
    expect(st.openExternal).not.toHaveBeenCalled();
    root.unmount();
  });

  it("landing on S3 with a linked wallet never opens checkout on its own", async () => {
    const st = spyStore(true);
    const { root } = await mount(<S3 store={st as unknown as Store} s={at("s3")} />);
    expect(st.onS3Pay).not.toHaveBeenCalled();
    root.unmount();
  });

  it("choosing membership shows the checkout with its price; Back returns to the free choice", async () => {
    const st = spyStore(false);
    const { host, root } = await mount(<S3 store={st as unknown as Store} s={at("s3")} />);
    const member = button(host, /Become a member/);
    expect(member).toBeTruthy();
    await act(async () => member!.click());
    // The checkout is the only place the price shows: the price, or (until core-membership's
    // price is read, #349/#379) the "See membership pricing" link in its place.
    expect(host.textContent ?? "").toMatch(/\$\s?\d|See membership pricing/);
    expect(button(host, /Link your wallet/)).toBeTruthy();
    expect(st.onS3Pay).not.toHaveBeenCalled();
    const back = button(host, /Back/);
    expect(back).toBeTruthy();
    await act(async () => back!.click());
    // Back ends any running pre-checkout grant check (core#584).
    expect(st.endS3Check).toHaveBeenCalledTimes(1);
    expect(button(host, /Run a node for free/)).toBeTruthy();
    expect(host.textContent ?? "").not.toMatch(PRICE);
    root.unmount();
  });

  it("C1-P: the free choice reads no price; only choosing membership starts the price read", async () => {
    const st = spyStore(false);
    const { host, root } = await mount(<S3 store={st as unknown as Store} s={at("s3")} />);
    expect(st.watchMembershipPrice).not.toHaveBeenCalled();
    const member = button(host, /Become a member/);
    await act(async () => member!.click());
    expect(st.watchMembershipPrice).toHaveBeenCalledTimes(1);
    root.unmount();
  });

  it("a member already in checkout (paying / settled) resumes on the checkout, not the choice", () => {
    for (const s3 of ["paying", "settled"] as const) {
      const text = textOf(renderToStaticMarkup(<S3 store={inertStore} s={at("s3", { s3 })} />));
      expect(text).not.toContain("Run a node for free");
    }
  });
});

describe("store.onS3Free — the free path runs a node", () => {
  beforeEach(() => {
    realStore.setState({ stage: "s3", s3: "idle", tier: "pilot", hasGrant: false });
  });

  it("continues onboarding to the wallet step on the free tier, with no checkout", () => {
    const checkout = vi.spyOn(bridge.membership, "checkout");
    realStore.onS3Free();
    expect(realStore.state.stage).toBe("s4");
    expect(realStore.state.tier).toBe("free");
    expect(realStore.state.s3).toBe("idle");
    expect(checkout).not.toHaveBeenCalled();
    checkout.mockRestore();
  });
});

describe("S4 — the free path skips the grant step and goes to node ignition", () => {
  it("free (no membership): Continue goes to S6, and says so", async () => {
    const st = spyStore(false);
    const { host, root } = await mount(<S4 store={st as unknown as Store} s={at("s4")} />);
    const go = button(host, /Continue/);
    expect(go?.textContent).not.toMatch(/grant/i);
    await act(async () => go!.click());
    expect(st.setState).toHaveBeenCalledWith({ stage: "s6" });
    root.unmount();
  });

  it("member (checkout settled): Continue still goes to the grant step S5", async () => {
    const st = spyStore(true);
    const { host, root } = await mount(<S4 store={st as unknown as Store} s={at("s4", { s3: "settled" })} />);
    await act(async () => button(host, /Continue/)!.click());
    expect(st.setState).toHaveBeenCalledWith({ stage: "s5" });
    root.unmount();
  });
});

describe("S4 / spine — a granted member is a member even before checkout state", () => {
  it("member (grant on chain, checkout state not settled): Continue goes to the grant step S5", async () => {
    const st = spyStore(true);
    const { host, root } = await mount(<S4 store={st as unknown as Store} s={at("s4", { s3: "idle", hasGrant: true })} />);
    const go = button(host, /Continue/);
    expect(go?.textContent).toMatch(/grant/i);
    await act(async () => go!.click());
    expect(st.setState).toHaveBeenCalledWith({ stage: "s5" });
    root.unmount();
  });

  /** The spine row (number badge, label, status) for the S5 grant step. */
  function s5Row(s: AppState): string {
    const host = document.createElement("div");
    host.innerHTML = renderToStaticMarkup(<Onboarding store={inertStore} s={s} />);
    const label = Array.from(host.querySelectorAll("span")).find((e) => /^(Grant \+ stake|Membership grant)$/.test(e.textContent ?? ""));
    expect(label).toBeTruthy();
    return label!.parentElement!.textContent ?? "";
  }

  it("free path past S5: the spine marks the grant step skipped, never done", () => {
    const row = s5Row(at("s6"));
    expect(row).toContain("skipped");
    expect(row).not.toContain("done");
    expect(row).not.toContain("✓");
  });

  it("member past S5 (settled or granted): the grant step is done", () => {
    for (const patch of [{ s3: "settled" as const }, { hasGrant: true }]) {
      const row = s5Row(at("s6", patch));
      expect(row).toContain("done");
      expect(row).not.toContain("skipped");
    }
  });
});

describe("TRIPWIRE matcher — what counts as a price figure", () => {
  it("catches dollar, USD and per-period figures", () => {
    for (const t of ["$48", "$ 48", "$48.00", "$0", "48 USD", "USD 48", "USD48", "48 / year", "48/year", "48 a year", "48 per year"]) {
      expect({ t, hit: PRICE.test(t) }).toEqual({ t, hit: true });
    }
  });

  it("ignores style and size values that happen to be 48", () => {
    for (const t of ["width: 48", "height={48}", "maxLength={48}", "48px", "padding: 48", "48 * 3600", "1 to 48 hours"]) {
      expect({ t, hit: PRICE.test(t) }).toEqual({ t, hit: false });
    }
  });
});

describe("S0 — nothing says paying is required to run a node", () => {
  it("says running a node is free and membership is optional", () => {
    const text = textOf(renderToStaticMarkup(<Onboarding store={inertStore} s={at("s0")} />));
    expect(text).toContain("Running a node is free");
    expect(text).not.toMatch(/One membership funds/);
    expect(text).not.toMatch(/Joining means a yearly membership/);
    expect(text).not.toMatch(PRICE);
  });
});

describe("TRIPWIRE — the price shows only on the member checkout", () => {
  it("no onboarding step renders a price figure outside the checkout", () => {
    const stages: AppState["stage"][] = ["s0", "s1", "s2", "s3", "s4", "s5", "s6", "s7"];
    for (const stage of stages) {
      const text = textOf(renderToStaticMarkup(<Onboarding store={inertStore} s={at(stage)} />));
      expect({ stage, price: PRICE.exec(text)?.[0] ?? null }).toEqual({ stage, price: null });
    }
  });

  it("in Onboarding.tsx, every price literal sits inside MembershipCheckout", () => {
    const src = readFileSync(join(__dirname, "Onboarding.tsx"), "utf8");
    const start = src.indexOf("function MembershipCheckout(");
    expect(start).toBeGreaterThan(-1);
    // The component ends where the next top-level function begins.
    const rest = src.slice(start + 1);
    const next = rest.search(/\n(export )?function /);
    const end = next === -1 ? src.length : start + 1 + next;
    const outside: string[] = [];
    const re = new RegExp(PRICE.source, "gi");
    for (let m = re.exec(src); m; m = re.exec(src)) {
      if (m.index < start || m.index >= end) outside.push(src.slice(Math.max(0, m.index - 40), m.index + 10).replace(/\s+/g, " "));
    }
    expect(outside).toEqual([]);
  });

  it("no other onboarding or shell source carries a price figure (comments included)", () => {
    const roots = [join(__dirname), join(__dirname, "..", "shell")];
    const hits: string[] = [];
    const walk = (dir: string) => {
      for (const name of readdirSync(dir)) {
        const p = join(dir, name);
        if (statSync(p).isDirectory()) walk(p);
        else if (/\.(ts|tsx)$/.test(name) && !/\.test\.tsx?$/.test(name) && name !== "Onboarding.tsx") {
          readFileSync(p, "utf8")
            .split("\n")
            .forEach((line, i) => {
              if (PRICE.test(line)) hits.push(`${p.split("/src/")[1]}:${i + 1}`);
            });
        }
      }
    };
    roots.forEach(walk);
    expect(hits).toEqual([]);
  });
});
