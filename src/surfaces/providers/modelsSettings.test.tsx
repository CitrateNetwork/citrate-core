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
  clockProblem: null,
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
    expect(c).toContain("gpt-a · no price yet");
    expect(c).toContain("gpt-b · no price yet");
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
    expect(card(html, "openai")).toContain("Paused automatically: 31 calls in 60 seconds");
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
    expect(html).toContain("refused: the call&#x27;s reservation would pass one of your caps; nothing was sent");
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

describe("Settings › Models: model prices under the default spending caps", () => {
  const priced = vendor({
    keyConfigured: true,
    listedModels: ["gpt-a", "gpt-b", "gpt-c"],
    models: [
      { id: "gpt-a", price: { inputMicrosPerMtok: 2_000_000, outputMicrosPerMtok: 8_000_000 }, source: "published", minReserveMicros: null },
      { id: "gpt-b", price: { inputMicrosPerMtok: 150_000, outputMicrosPerMtok: 600_000 }, source: "member", minReserveMicros: null },
      { id: "gpt-c", price: null, source: null, minReserveMicros: null },
    ],
  });

  it("each model shows its price and where it came from, or that it has none", () => {
    const c = card(render({ statuses: [priced] }), "openai");
    expect(c).toContain("gpt-a · $2.00 in / $8.00 out per 1M tokens (published price)");
    expect(c).toContain("gpt-b · $0.15 in / $0.60 out per 1M tokens (price you entered)");
    expect(c).toContain("gpt-c · no price yet");
  });

  it("with a spending cap set, unpriced models are said to be refused, with a way to enter a price", () => {
    const c = card(render({ statuses: [priced] }), "openai");
    expect(c).toContain('data-testid="unpriced-warning"');
    expect(c).toContain("1 model has no price. While a spending cap is set, calls to it are refused");
    expect(c).toContain('data-testid="enter-price-gpt-c"');
    expect(c).toMatch(/data-testid="enter-price-gpt-c"[^>]*>Enter price</);
    expect(c).toMatch(/data-testid="enter-price-gpt-b"[^>]*>Change price</);
    expect(c).toMatch(/data-testid="enter-price-gpt-a"[^>]*>Override price</);
  });

  it("with no spending cap, an unpriced model is not called refused", () => {
    const noMoney = { ...priced, caps: { ...priced.caps, dailyMicros: null, monthlyMicros: null } };
    const c = card(render({ statuses: [noMoney] }), "openai");
    expect(c).not.toContain("unpriced-warning");
    expect(c).toContain("gpt-c · no price yet");
  });

  it("a recent call refused for an unknown price says why and what to do", () => {
    const recentCalls = [{ atMs: NOW, provider: "openai", model: "gpt-c", verified: true, outcome: "price_unknown", refused: true, tokensIn: null, tokensOut: null, costMicros: null }];
    const html = render({ statuses: [priced], usage: summary({ recentCalls }) });
    expect(html).toContain("refused: this model has no price, so its cost under your spending cap is unknown. Enter its price on the OpenAI card");
  });

  it("other refusal and failure codes read as plain words", () => {
    const mk = (outcome: string, refused: boolean) => ({ atMs: NOW, provider: "openai", model: "m", verified: true, outcome, refused, tokensIn: null, tokensOut: null, costMicros: null });
    const html = render({ statuses: [priced], usage: summary({ recentCalls: [mk("cap_exceeded", true), mk("halted", true), mk("key_rejected", false), mk("log_unavailable", true)] }) });
    expect(html).toContain("refused: the call&#x27;s reservation would pass one of your caps; nothing was sent");
    expect(html).toContain("refused: the provider was paused");
    expect(html).toContain("failed: the vendor rejected the key");
    expect(html).toContain("refused: the usage log could not be used");
  });
});

describe("Settings › Models: fail-closed states pause every provider", () => {
  const unreadable = { kind: "guardUnreadable", message: "paused: the spend settings file could not be verified; check your caps and resume", atMs: 0 };

  it("an unverifiable or rolled-back settings file: one page banner, every provider paused, acknowledge resets to defaults", () => {
    const html = render({ statuses: [vendor({ keyConfigured: true, halt: unreadable }), vendor({ id: "xai", name: "xAI", halt: unreadable })] });
    expect(html).toContain('data-testid="failclosed-guard"');
    expect(html).toContain("Every provider is paused");
    expect(html).toContain("an older copy than the one last saved");
    expect(html).toContain("Acknowledge and reset settings");
    expect(html).toContain("Acknowledging resets, for every provider");
    expect(html.match(/data-testid="failclosed-guard"/g)?.length).toBe(1);
  });

  it("a halt with no recorded time shows no fake time", () => {
    const html = render({ statuses: [vendor({ keyConfigured: true, halt: unreadable })] });
    expect(html).not.toContain("1970-01-01");
    expect(html).not.toMatch(/Paused :/);
  });

  it("a log cut short or removed: every call is refused, the reset keeps this month's spend", () => {
    const html = render({ statuses: [vendor({ keyConfigured: true })], usage: summary({ logIntact: false, logProblem: "the usage log failed verification (the file is shorter than its anchor)" }) });
    expect(html).toContain('data-testid="usage-log-broken"');
    expect(html).toContain("the file is shorter than its anchor");
    expect(html).toContain("This month&#x27;s spend carries into the new log, so starting one does not reset your caps.");
  });

  it("a clock moved back: every call is refused until the clock is right, with a check-again action", () => {
    const clockProblem = "this computer's clock reads earlier than the last recorded call; check the date and time";
    const html = render({ statuses: [vendor({ keyConfigured: true })], usage: summary({ clockProblem }) });
    expect(html).toContain('data-testid="failclosed-clock"');
    expect(html).toContain("Every cloud call is refused");
    expect(html).toContain("clock reads earlier than the last recorded call");
    expect(html).toContain("Check again");
    expect(render()).not.toContain("failclosed-clock");
  });
});

describe("Settings › Models: vendor-specific key guidance (core #530)", () => {
  it("Anthropic steers toward a workspace-scoped, spend-limited key", () => {
    const html = render({ statuses: [vendor({ id: "anthropic", name: "Anthropic", host: "api.anthropic.com", adapter: "native" })] });
    const c = card(html, "anthropic");
    expect(c).toContain("workspace-scoped key");
    expect(c).toContain("spend limit");
    expect(c).toContain("native adapter");
  });

  it("Gemini says there is no fixed key format, so the live test decides", () => {
    const c = card(render({ statuses: [vendor({ id: "gemini", name: "Google Gemini", host: "generativelanguage.googleapis.com", adapter: "native" })] }), "gemini");
    expect(c).toContain("no fixed key format");
    expect(c).toContain("the live test with Google decides");
  });
});

describe("Settings › Models: a large reservation per call (xAI)", () => {
  const grok = { id: "grok-4.7", price: { inputMicrosPerMtok: 3_000_000, outputMicrosPerMtok: 12_000_000 }, source: "published" as const, minReserveMicros: 6_000_050 };
  const xai = (dailyMicros: number | null, keyConfigured = false) =>
    vendor({ id: "xai", name: "xAI", host: "api.x.ai", adapter: "native", keyConfigured, models: [grok], caps: { dailyTokens: 1_000_000, monthlyTokens: 10_000_000, dailyMicros, monthlyMicros: 50_000_000 } });

  it("before connecting, the card says why the reservation is large and roughly what one call reserves", () => {
    const c = card(render({ statuses: [xai(20_000_000)] }), "xai");
    expect(c).toContain('data-testid="reserve-note-grok-4.7"');
    expect(c).toContain("Each grok-4.7 call reserves about $6.00 against your caps before it is sent");
    expect(c).toContain("xAI documents no limit on reasoning output, so core reserves the model&#x27;s full context window");
    expect(c).toContain("the reservation settles to the cost of the tokens xAI reports, at the price shown here");
    expect(c).not.toContain("the most the vendor can bill");
    expect(c).toContain("key-hygiene");
  });

  it("with the daily cap above the reservation there is no refusal warning and no raise action", () => {
    const c = card(render({ statuses: [xai(20_000_000)] }), "xai");
    expect(c).not.toContain("reserve-refused");
    expect(c).not.toContain("Raise the xAI daily cap");
  });

  it("with the daily cap below the reservation, it says every call is refused and offers to raise the cap", () => {
    const c = card(render({ statuses: [xai(5_000_000, true)] }), "xai");
    expect(c).toContain('data-testid="reserve-refused-grok-4.7"');
    expect(c).toContain("That is above your $5.00 daily cap, so every grok-4.7 call is refused and nothing is sent");
    expect(c).toContain("Raise the xAI daily cap");
  });

  it("a small reservation is not called out", () => {
    const small = vendor({ keyConfigured: true, models: [{ id: "gpt-a", price: { inputMicrosPerMtok: 1, outputMicrosPerMtok: 1 }, source: "published", minReserveMicros: 900 }] });
    expect(card(render({ statuses: [small] }), "openai")).not.toContain("reserve-note-");
  });
});

describe("Settings › Models: review fixes (#531)", () => {
  it("the key box asks password managers not to fill or save it", () => {
    expect(card(render(), "openai")).toMatch(/<input[^>]*type="password"[^>]*autocomplete="new-password"|<input[^>]*autocomplete="new-password"[^>]*type="password"/i);
  });

  it("the settings-reset acknowledge lists everything that resets", () => {
    const unreadable = { kind: "guardUnreadable", message: "paused: the spend settings file could not be verified; check your caps and resume", atMs: 0 };
    const html = render({ statuses: [vendor({ keyConfigured: true, halt: unreadable })] });
    expect(html).toContain("caps, model prices, pauses, approved and listed models, custom endpoints, and the dates your keys were stored");
    expect(html).toContain("Your keys stay in the keychain");
  });

  it("a halt reads once as paused, with its time", () => {
    const halt = { kind: "burst", message: "paused automatically: 31 calls in 60 seconds", atMs: NOW };
    const html = render({ statuses: [vendor({ keyConfigured: true, halt })] });
    expect(html).not.toMatch(/Paused[^<]*: paused/i);
    expect(html).toContain("Paused automatically: 31 calls in 60 seconds (since 2026-10-26 01:00 UTC)");
  });
});
