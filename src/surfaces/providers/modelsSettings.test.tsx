// D-11 (RC52) — Settings › Models renders only what core reports, in every state a provider can be
// in: connected, testing, failed test, halted (with what Resume does), capped, not available in this
// build, an Unverified custom endpoint before and after confirmation, and the web preview. Usage
// and recent calls come only from the usage summary; both token and spending caps are shown.
import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { ModelsSettingsView, type ModelsActions, type ModelsViewProps } from "./ModelsSettings";
import { EMPTY_CUSTOM, capsToForm } from "./providerModels";
import type { ProviderUsageSummary } from "../../bridge/domains";
import { DAY, NOW, usage, vendor } from "./fixtures/providerStates.fixture";

const noop = () => {};
const on = new Proxy({}, { get: () => noop }) as ModelsActions;

const summary = (over: Partial<ProviderUsageSummary> = {}): ProviderUsageSummary => ({
  logIntact: true,
  logProblem: null,
  records: 4,
  asOfMs: NOW,
  providers: [usage()],
  recentCalls: [],
  ...over,
});

const base: ModelsViewProps = {
  desktop: true,
  statuses: [vendor()],
  usage: summary(),
  loadError: null,
  nowMs: NOW,
  busy: null,
  errors: {},
  notices: {},
  keyDrafts: {},
  confirmDrafts: {},
  capsEdit: null,
  priceEdit: null,
  advanced: false,
  customForm: EMPTY_CUSTOM,
  on,
};

const render = (p: Partial<ModelsViewProps> = {}) => renderToStaticMarkup(<ModelsSettingsView {...base} {...p} />);
const card = (html: string, id: string) => {
  const start = html.indexOf(`data-testid="provider-card-${id}"`);
  expect(start).toBeGreaterThan(-1);
  const ends = [html.indexOf('data-testid="provider-card-', start + 10), html.indexOf("Custom endpoints", start)].filter((i) => i > -1);
  return html.slice(start, ends.length ? Math.min(...ends) : undefined);
};
const stateOf = (html: string, id: string) => /data-state="([a-z]+)"/.exec(card(html, id))?.[1];

const SIX = [
  vendor(),
  vendor({ id: "anthropic", name: "Anthropic", host: "api.anthropic.com", adapter: null }),
  vendor({ id: "gemini", name: "Google Gemini", host: "generativelanguage.googleapis.com", adapter: null }),
  vendor({ id: "xai", name: "xAI", host: "api.x.ai" }),
  vendor({ id: "deepseek", name: "DeepSeek", host: "api.deepseek.com" }),
  vendor({ id: "meta", name: "Meta", host: "api.llama.com", adapter: null }),
];

describe("Settings › Models: the vendor cards", () => {
  it("one card per vendor core reports, plus Cognition as 'Delegate to Devin', not available yet", () => {
    const html = render({ statuses: SIX });
    for (const v of SIX) expect(html).toContain(`provider-card-${v.id}`);
    expect(html).toContain("Muse models on the Meta Model API");
    expect(card(html, "cognition")).toContain("Delegate to Devin");
    expect(card(html, "cognition")).toContain("not available in this build");
    expect(card(html, "cognition")).not.toMatch(/<input/);
  });

  it("a vendor with no adapter in this build says so and offers no key box", () => {
    const html = render({ statuses: SIX });
    const a = card(html, "anthropic");
    expect(stateOf(html, "anthropic")).toBe("unavailable");
    expect(a).toContain("Not available in this build");
    expect(a).not.toContain("API key");
    expect(a).not.toContain(">Connect<");
  });

  it("disconnected: the key-hygiene guide with the vendor's key page, and a one-way key box", () => {
    const html = render();
    const c = card(html, "openai");
    expect(stateOf(html, "openai")).toBe("disconnected");
    expect(c).toContain("key-hygiene");
    expect(c).toContain("used only by Citrate Core");
    expect(c).toContain("monthly spending limit");
    expect(c).toContain("Open the OpenAI key page");
    expect(c).toContain("never sent to Citrate");
    expect(c).toMatch(/<input[^>]*type="password"/);
    expect(c).toMatch(/<button[^>]*disabled=""[^>]*>Connect</);
  });

  it("testing: says which pinned host the key is tested with and that it is stored only if the test passes", () => {
    const html = render({ busy: { id: "openai", op: "connect" }, keyDrafts: { openai: "x".repeat(30) } });
    expect(card(html, "openai")).toContain("Testing the key with api.openai.com. It is stored only if the test passes.");
  });

  it("connected: key in the keychain, test, remove here, and remove here plus the vendor revoke page", () => {
    const html = render({ statuses: [vendor({ keyConfigured: true, keySetMs: NOW - 3 * DAY, listedModels: ["gpt-a", "gpt-b"] })] });
    const c = card(html, "openai");
    expect(stateOf(html, "openai")).toBe("connected");
    expect(c).toContain("Key in this computer&#x27;s keychain");
    expect(c).toContain("It is never shown again");
    expect(c).toContain("Test key");
    expect(c).toContain("Remove key here");
    expect(c).toContain("Remove here and open the OpenAI revoke page");
    expect(c).toContain("still works at OpenAI until you revoke it there");
    expect(c).toContain("Models: gpt-a, gpt-b");
    expect(c).not.toContain("rotation-reminder");
    expect(c).not.toContain("key-hygiene");
  });

  it("model choice for Hermes is honestly not available yet", () => {
    const html = render({ statuses: [vendor({ keyConfigured: true, listedModels: ["gpt-a"] })] });
    expect(card(html, "openai")).toContain("Choosing which of these models Hermes uses is not available yet");
  });

  it("an old key gets a rotation reminder", () => {
    const html = render({ statuses: [vendor({ keyConfigured: true, keySetMs: NOW - 120 * DAY })] });
    expect(card(html, "openai")).toContain("This key is 120 days old");
  });

  it("failed test: the core error is shown on the card", () => {
    const html = render({ statuses: [vendor({ keyConfigured: true })], errors: { openai: "The key test failed: the vendor rejected the key" } });
    expect(card(html, "openai")).toContain('role="alert"');
    expect(card(html, "openai")).toContain("The key test failed: the vendor rejected the key");
  });

  it("halted: the card names the pause and points to Resume", () => {
    const halt = { kind: "burst", message: "paused automatically: 31 calls in 60 seconds", atMs: NOW };
    const html = render({ statuses: [vendor({ keyConfigured: true, halt })] });
    expect(stateOf(html, "openai")).toBe("halted");
    expect(card(html, "openai")).toContain("paused automatically: 31 calls in 60 seconds");
  });

  it("capped: the card says which cap is reached", () => {
    const u = usage({ today: { calls: 9, tokens: 10, spendMicros: 5_000_000, unpricedCalls: 0 } });
    const html = render({ statuses: [vendor({ keyConfigured: true })], usage: summary({ providers: [u] }) });
    expect(stateOf(html, "openai")).toBe("capped");
    expect(card(html, "openai")).toContain("The daily spending cap is reached");
  });

  it("no em-dashes, and never a key, in any state", () => {
    const html = render({ statuses: [...SIX, vendor({ id: "custom-1", name: "Box", kind: "custom", verified: false, confirmed: false, keyPage: null, revokePage: null })], advanced: true });
    expect(html).not.toContain("—");
    expect(html).not.toMatch(/sk-[A-Za-z0-9]/);
  });
});

describe("Settings › Models: custom endpoints (Advanced, Unverified)", () => {
  const custom = vendor({ id: "custom-ab12", name: "Box", kind: "custom", verified: false, baseUrl: "https://llm.example.org/v1", host: "llm.example.org", confirmed: false, keyPage: null, revokePage: null });

  it("the add form is behind the Advanced toggle, with a warning", () => {
    expect(render()).not.toContain("custom-advanced");
    const html = render({ advanced: true });
    expect(html).toContain("custom-warning");
    expect(html).toContain("labelled Unverified everywhere");
    expect(html).toContain("Add unverified endpoint");
  });

  it("an unconfirmed endpoint is Unverified, refuses calls and asks for the exact URL", () => {
    const html = render({ statuses: [vendor(), custom] });
    const c = card(html, "custom-ab12");
    expect(stateOf(html, "custom-ab12")).toBe("unconfirmed");
    expect(c).toContain("unverified-badge");
    expect(c).toContain("Every call to this endpoint is refused until you confirm it");
    expect(c).toMatch(/<button[^>]*disabled=""[^>]*>Confirm endpoint/);
    expect(c).not.toMatch(/type="password"/);
  });

  it("the Confirm button only enables on the exact URL", () => {
    const near = render({ statuses: [custom], confirmDrafts: { "custom-ab12": "https://llm.example.org/v1/" } });
    expect(card(near, "custom-ab12")).toMatch(/<button[^>]*disabled=""[^>]*>Confirm endpoint/);
    const exact = render({ statuses: [custom], confirmDrafts: { "custom-ab12": "https://llm.example.org/v1" } });
    expect(card(exact, "custom-ab12")).not.toMatch(/<button[^>]*disabled=""[^>]*>Confirm endpoint/);
  });

  it("a confirmed endpoint is still Unverified on its card, in usage and in recent calls", () => {
    const ok = { ...custom, confirmed: true, keyConfigured: true };
    const recent = { atMs: NOW, provider: "custom-ab12", model: "m", verified: false, outcome: "ok", refused: false, tokensIn: 1, tokensOut: 2, costMicros: null };
    const html = render({ statuses: [ok], usage: summary({ providers: [usage({ id: "custom-ab12" })], recentCalls: [recent] }) });
    expect(card(html, "custom-ab12")).toContain("unverified-badge");
    expect(card(html, "custom-ab12")).toContain("not a governed vendor");
    expect(html).toMatch(/usage-row-custom-ab12[\s\S]*unverified-badge/);
    expect(html).toContain("Box (Unverified)");
  });
});

describe("Settings › Models: spend and usage", () => {
  const connected = vendor({ keyConfigured: true });

  it("shows today and month against both the token and the spending caps", () => {
    const u = usage({ today: { calls: 2, tokens: 1200, spendMicros: 30_000, unpricedCalls: 0 }, month: { calls: 7, tokens: 9000, spendMicros: 410_000, unpricedCalls: 1 } });
    const html = render({ statuses: [connected], usage: summary({ providers: [u] }) });
    expect(html).toContain("Today: 2 calls · 1,200 of 1,000,000 tokens · $0.03 of $5.00 spent");
    expect(html).toContain("This month: 7 calls · 9,000 of 10,000,000 tokens · $0.41 of $50.00 spent, plus 1 call with no price set (cost unknown)");
    expect(html).toContain("Log verified · 4 records");
  });

  it("unsettled calls are said, with their reservation still counting", () => {
    const html = render({ statuses: [connected], usage: summary({ providers: [usage({ unsettledCalls: 2 })] }) });
    expect(html).toContain("2 calls have not finished");
  });

  it("halted unknown model: the reason, and that Resume approves the model", () => {
    const halt = { kind: "unknownModel", message: 'paused automatically: a call asked for model "gpt-x", which this provider has not listed; resuming approves it', atMs: NOW };
    const html = render({ statuses: [vendor({ keyConfigured: true, halt })] });
    expect(html).toContain("halt-block");
    expect(html).toContain('a call asked for model &quot;gpt-x&quot;');
    expect(html).toContain("Resume and approve this model");
    expect(html).toContain("Resuming approves the model named above");
  });

  it("a member pause resumes plainly; a connected provider can be paused", () => {
    const paused = render({ statuses: [vendor({ keyConfigured: true, halt: { kind: "member", message: "paused by you", atMs: NOW } })] });
    expect(paused).toMatch(/>Resume</);
    expect(render({ statuses: [connected] })).toContain("Pause this provider");
  });

  it("the caps editor has token and spending caps, blank meaning no cap", () => {
    const html = render({ statuses: [connected], capsEdit: { id: "openai", form: capsToForm(connected.caps) } });
    for (const l of ["daily token cap", "monthly token cap", "daily spending cap", "monthly spending cap"]) expect(html).toContain(`aria-label="${l}"`);
    expect(html).toContain('value="5.00"');
    expect(html).toContain('value="50.00"');
    expect(html).toContain('value="1000000"');
    expect(html).toContain("Leave a box blank for no cap");
    expect(html).toContain("calls to a model with no price are refused");
  });

  it("a broken usage log says every call is refused and offers a new log", () => {
    const html = render({ statuses: [connected], usage: summary({ logIntact: false, logProblem: "line 3 does not match the chain" }) });
    expect(html).toContain("usage-log-broken");
    expect(html).toContain("line 3 does not match the chain");
    expect(html).toContain("Every cloud call is refused");
    expect(html).toContain("Start a new usage log");
    expect(html).not.toContain("Log verified");
  });

  it("recent calls show metadata only, unknown cost as unknown, and refusals as refusals", () => {
    const recentCalls = [
      { atMs: NOW, provider: "openai", model: "gpt-a", verified: true, outcome: "cap_exceeded", refused: true, tokensIn: null, tokensOut: null, costMicros: null },
      { atMs: NOW - 1000, provider: "openai", model: "gpt-a", verified: true, outcome: null, refused: false, tokensIn: null, tokensOut: null, costMicros: null },
      { atMs: NOW - 2000, provider: "openai", model: "gpt-a", verified: true, outcome: "ok", refused: false, tokensIn: 12, tokensOut: 5, costMicros: 1500 },
    ];
    const html = render({ statuses: [connected], usage: summary({ recentCalls }) });
    expect(html).toContain("no prompts or replies");
    expect(html).toContain("refused: cap exceeded");
    expect(html).toContain("not finished");
    expect(html).toContain("12 in / 5 out · $0.0015 · ok");
    expect(html).toContain("tokens not reported · cost unknown");
  });

  it("with nothing connected, usage says so instead of showing zero rows", () => {
    expect(render()).toContain("No provider is connected, so nothing has been used.");
  });
});

describe("Settings › Models: the web preview", () => {
  it("says vendors connect only in the desktop app, and lists no fabricated providers", () => {
    const html = render({ desktop: false, statuses: null, usage: null });
    expect(html).toContain("This web preview has no OS keychain");
    expect(html).toContain("Not available in the web preview.");
    expect(html).not.toContain("provider-card-openai");
    expect(html).not.toContain("usage-row-");
  });

  it("a failed status read is shown as such, not as an empty list", () => {
    const html = render({ statuses: null, usage: null, loadError: "the model proxy could not start: AddrInUse" });
    expect(html).toContain("Provider status could not be read: the model proxy could not start: AddrInUse");
  });
});
