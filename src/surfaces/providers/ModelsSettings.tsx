// =====================================================================
// D-11 (RC52) — Settings › Models (Hermes › Controls › Details › Models).
//
// One card per governed vendor core reports (OpenAI, Anthropic, Google Gemini, xAI, DeepSeek, Meta),
// a Cognition "Delegate to Devin" card that says it is not available yet, custom endpoints behind
// an Advanced toggle (Unverified everywhere), and the Spend and usage view.
//
// Data sources (Rule 7), all through `bridge.providers` (Rust `providers_ipc.rs`):
//   cards, caps, halts, models, prices, key age  -> providers_status (pinned vendor table, keychain
//                                                   presence, sealed guard file)
//   spend, usage, recent calls, log state        -> providers_usage_summary (the signed usage log)
//   connect / test                               -> providers_key_set / providers_key_test (a live,
//                                                   free vendor call through the pinned transport)
//   key and revoke pages                         -> the pinned vendor table (keyPage / revokePage)
// A key typed here goes to core once and is never shown again (ADR P-2). The web preview has no
// keychain: the bridge reports Unavailable and this page says so instead of listing anything.
//
// The view is a pure function of its props (render-tested); the container loads and acts.
// =====================================================================
import { useEffect, useState } from "react";
import { bridge } from "../../bridge";
import { BRIDGE_MODE } from "../../bridge/mode";
import type { ProviderModelPrice, ProviderRecentCall, ProviderStatus, ProviderUsage, ProviderUsageSummary } from "../../bridge/domains";
import { formatMicros, parseUsdToMicros } from "../../agent/escalation";
import {
  EMPTY_CUSTOM,
  VENDOR_HINT,
  VENDOR_SUBTITLE,
  capReached,
  capsToForm,
  cardState,
  confirmMatches,
  customToInput,
  fmtDay,
  fmtTokens,
  fmtWhen,
  formToCaps,
  hygieneSteps,
  resumeEffect,
  rotationDue,
  spendLine,
  tokenLine,
  type CapsForm,
  type CardState,
  type CustomForm,
} from "./providerModels";

export type BusyOp = "connect" | "test" | "remove" | "revoke" | "halt" | "resume" | "caps" | "price" | "confirm" | "customRemove" | "custom" | "reset";

export interface PriceForm {
  model: string;
  inUsd: string;
  outUsd: string;
}

export interface ModelsActions {
  onKeyDraft(id: string, v: string): void;
  onConnect(id: string): void;
  onTest(id: string): void;
  onRemove(id: string): void;
  onRemoveAndRevoke(id: string): void;
  onOpen(url: string): void;
  onHalt(id: string): void;
  onResume(id: string): void;
  onEditCaps(id: string | null): void;
  onCapsForm(f: CapsForm): void;
  onSaveCaps(): void;
  onEditPrice(id: string | null, model?: string): void;
  onPriceForm(f: PriceForm): void;
  onSavePrice(): void;
  onConfirmDraft(id: string, v: string): void;
  onConfirm(id: string): void;
  onRemoveCustom(id: string): void;
  onAdvanced(on: boolean): void;
  onCustomForm(f: CustomForm): void;
  onAddCustom(): void;
  onResetLog(): void;
  /** Read status and usage again (after the member fixes something outside the app). */
  onRefresh(): void;
}

export interface ModelsViewProps {
  desktop: boolean;
  statuses: ProviderStatus[] | null;
  usage: ProviderUsageSummary | null;
  /** Why the status or usage could not be read (shown as is). */
  loadError: string | null;
  nowMs: number;
  busy: { id: string; op: BusyOp } | null;
  /** Per provider id; the key "" is for page-level errors. */
  errors: Record<string, string>;
  /** Per provider id: the result of the last test or connect. */
  notices: Record<string, string>;
  keyDrafts: Record<string, string>;
  confirmDrafts: Record<string, string>;
  capsEdit: { id: string; form: CapsForm } | null;
  priceEdit: { id: string; form: PriceForm } | null;
  advanced: boolean;
  customForm: CustomForm;
  on: ModelsActions;
}

const STATE_CHIP: Record<CardState, { text: string; color: string }> = {
  unavailable: { text: "not available yet", color: "var(--tx-3)" },
  unconfirmed: { text: "waiting for your confirmation", color: "var(--warn)" },
  halted: { text: "paused", color: "var(--warn)" },
  capped: { text: "cap reached", color: "var(--warn)" },
  connected: { text: "connected", color: "var(--ok)" },
  disconnected: { text: "not connected", color: "var(--tx-3)" },
};

const ADAPTER_LABEL: Record<string, string> = {
  native: "native adapter",
  "openai-compatible": "OpenAI-compatible adapter",
};

const small = { fontSize: 10.5, color: "var(--tx-3)" } as const;
const row = { display: "flex", gap: 8, alignItems: "center", flexWrap: "wrap" } as const;
const field = { height: 30, fontSize: 12 } as const;

function UnverifiedBadge() {
  return (
    <span className="mono" data-testid="unverified-badge" style={{ fontSize: 9.5, color: "var(--warn)", border: "1px solid var(--warn)", borderRadius: 4, padding: "0 5px", marginLeft: 8 }}>
      Unverified
    </span>
  );
}

/** Plain words for the usage log's coarse outcome codes (`ProxyError::outcome` in core). */
const OUTCOME_WORDS: Record<string, string> = {
  cap_exceeded: "a cap was reached",
  halted: "the provider was paused",
  log_unavailable: "the usage log could not be used",
  egress_refused: "the address is not this provider's pinned host",
  not_configured: "no key is connected",
  not_available: "this build has no adapter for it",
  key_rejected: "the vendor rejected the key",
  rate_limited: "the vendor is limiting requests",
  redirect_refused: "the vendor redirected off its host",
  provider_error: "the vendor returned an error",
};

function outcomeText(c: ProviderRecentCall, providerName: string): string {
  if (c.outcome === null) return "not finished (in flight, or interrupted; its reservation still counts)";
  if (c.outcome === "ok") return "ok";
  const prefix = c.refused ? "refused: " : "failed: ";
  if (c.outcome === "price_unknown") return `${prefix}this model has no price, so its cost under your spending cap is unknown. Enter its price on the ${providerName} card`;
  return prefix + (OUTCOME_WORDS[c.outcome] ?? c.outcome.replace(/_/g, " "));
}

/** "Paused 2026-10-26 01:00 UTC" or just "Paused" when core recorded no time. */
const pausedAt = (atMs: number) => (atMs > 0 ? `Paused ${fmtWhen(atMs)}` : "Paused");

const priceText = (m: ProviderModelPrice) =>
  m.price
    ? `${m.id} · ${formatMicros(m.price.inputMicrosPerMtok)} in / ${formatMicros(m.price.outputMicrosPerMtok)} out per 1M tokens (${m.source === "member" ? "price you entered" : "published price"})`
    : `${m.id} · no price yet`;

function ProviderCard(props: { p: ProviderStatus; u: ProviderUsage | undefined; v: ModelsViewProps }) {
  const { p, u, v } = props;
  const st = cardState(p, u);
  const chip = STATE_CHIP[st];
  const busyOp = v.busy && v.busy.id === p.id ? v.busy.op : null;
  const anyBusy = v.busy !== null;
  const err = v.errors[p.id];
  const notice = v.notices[p.id];
  const custom = p.kind === "custom";
  const age = rotationDue(p, v.nowMs);
  const draft = v.keyDrafts[p.id] ?? "";
  // Core's model list with price sources; models it listed but did not price yet are added unpriced.
  const priced: ProviderModelPrice[] = [...p.models];
  for (const id of [...p.listedModels, ...p.approvedModels]) if (!priced.some((m) => m.id === id)) priced.push({ id, price: null, source: null });
  const unpriced = priced.filter((m) => m.price === null).length;
  const moneyCap = p.caps.dailyMicros !== null || p.caps.monthlyMicros !== null;
  const pe = v.priceEdit && v.priceEdit.id === p.id ? v.priceEdit.form : null;
  const set = (k: keyof PriceForm) => (e: { target: { value: string } }) => pe && v.on.onPriceForm({ ...pe, [k]: e.target.value });

  return (
    <div data-testid={`provider-card-${p.id}`} data-state={st} style={{ display: "flex", flexDirection: "column", gap: 8, padding: "14px 18px", borderBottom: "1px solid var(--line-1)" }}>
      <span style={{ display: "flex", alignItems: "baseline", flexWrap: "wrap", gap: 4 }}>
        <span style={{ fontSize: 13, fontWeight: 500 }}>{p.name || p.id}</span>
        {custom && <UnverifiedBadge />}
        <span className="mono" data-testid="provider-state" style={{ fontSize: 9.5, color: chip.color, marginLeft: 8 }}>
          {chip.text}
        </span>
      </span>
      <span className="mono" style={small}>
        {VENDOR_SUBTITLE[p.id] ? VENDOR_SUBTITLE[p.id] + " · " : ""}
        {custom ? `${p.baseUrl} · not a governed vendor` : `${p.host} (pinned)`}
        {p.adapter ? " · " + (ADAPTER_LABEL[p.adapter] ?? p.adapter) : ""}
      </span>

      {st === "unavailable" && (
        <span style={{ fontSize: 12, color: "var(--tx-2)" }}>
          Not available in this build: core has no adapter for {p.name || p.id} yet, so it cannot be connected.
        </span>
      )}

      {st === "unconfirmed" && (
        <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
          <span style={{ fontSize: 12, color: "var(--warn)" }}>
            Every call to this endpoint is refused until you confirm it. Type the exact URL to confirm you trust it.
          </span>
          <span className="mono" style={{ fontSize: 11.5 }}>{p.baseUrl}</span>
          <span style={row}>
            <input
              className="input mono"
              aria-label={`confirm the URL for ${p.name}`}
              placeholder="type the URL exactly"
              value={v.confirmDrafts[p.id] ?? ""}
              onChange={(e) => v.on.onConfirmDraft(p.id, e.target.value)}
              style={{ ...field, width: 280 }}
              disabled={anyBusy}
            />
            <button className="btn btn-secondary btn-sm" onClick={() => v.on.onConfirm(p.id)} disabled={anyBusy || !confirmMatches(v.confirmDrafts[p.id] ?? "", p.baseUrl)}>
              {busyOp === "confirm" ? "Confirming" : "Confirm endpoint"}
            </button>
          </span>
        </div>
      )}

      {p.halt && (
        <span data-testid="provider-halt-line" style={{ fontSize: 12, color: "var(--warn)" }}>
          {pausedAt(p.halt.atMs)}: {p.halt.message}. Resume it under Spend and usage.
        </span>
      )}
      {st === "capped" && (
        <span data-testid="provider-cap-line" style={{ fontSize: 12, color: "var(--warn)" }}>
          {capReached(u, p.caps)}
        </span>
      )}

      {st !== "unavailable" && st !== "unconfirmed" && p.keyConfigured && (
        <>
          <span className="mono" style={{ fontSize: 11, color: "var(--ok)" }}>
            Key in this computer's keychain{p.keySetMs ? ", stored " + fmtDay(p.keySetMs) : ""}. It is never shown again.
          </span>
          {busyOp === "test" && <span className="mono" style={small}>Testing the key with {p.host}</span>}
          {age !== null && (
            <span data-testid="rotation-reminder" style={{ fontSize: 12, color: "var(--warn)" }}>
              This key is {age} days old. Make a new one at the vendor, connect it here, then revoke the old one.
            </span>
          )}
          <span style={row}>
            <button className="btn btn-ghost btn-sm" onClick={() => v.on.onTest(p.id)} disabled={anyBusy || !v.desktop}>
              {busyOp === "test" ? "Testing" : "Test key"}
            </button>
            {custom ? (
              <button className="btn btn-danger btn-sm" onClick={() => v.on.onRemoveCustom(p.id)} disabled={anyBusy}>
                Remove endpoint and key
              </button>
            ) : (
              <>
                <button className="btn btn-danger btn-sm" onClick={() => v.on.onRemove(p.id)} disabled={anyBusy}>
                  Remove key here
                </button>
                {p.revokePage && (
                  <button className="btn btn-ghost btn-sm" data-testid="remove-and-revoke" onClick={() => v.on.onRemoveAndRevoke(p.id)} disabled={anyBusy}>
                    Remove here and open the {p.name} revoke page
                  </button>
                )}
              </>
            )}
          </span>
          {!custom && (
            <span className="mono" style={small}>
              Removing here deletes the key from this computer. The key still works at {p.name} until you revoke it there.
            </span>
          )}
        </>
      )}

      {st !== "unavailable" && st !== "unconfirmed" && !p.keyConfigured && (
        <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
          {!custom && (
            <div data-testid="key-hygiene" style={{ display: "flex", flexDirection: "column", gap: 3 }}>
              <span className="eyebrow">Before you connect</span>
              <ol style={{ margin: 0, paddingLeft: 18, fontSize: 11.5, color: "var(--tx-2)", lineHeight: 1.5 }}>
                {hygieneSteps(p.name, p.host).map((s) => (
                  <li key={s}>{s}</li>
                ))}
              </ol>
              {VENDOR_HINT[p.id] && <span style={{ fontSize: 11.5, color: "var(--tx-2)" }}>{VENDOR_HINT[p.id]}</span>}
              {p.keyPage && (
                <span>
                  <button className="btn btn-ghost btn-sm" onClick={() => v.on.onOpen(p.keyPage as string)}>
                    Open the {p.name} key page
                  </button>
                </span>
              )}
            </div>
          )}
          <span style={row}>
            <input
              className="input mono"
              type="password"
              autoComplete="off"
              aria-label={`${p.name} API key`}
              placeholder="paste the API key"
              value={draft}
              onChange={(e) => v.on.onKeyDraft(p.id, e.target.value)}
              style={{ ...field, width: 260 }}
              disabled={!v.desktop || anyBusy}
            />
            <button className="btn btn-secondary btn-sm" onClick={() => v.on.onConnect(p.id)} disabled={!v.desktop || anyBusy || draft.trim() === ""}>
              {busyOp === "connect" ? "Testing" : "Connect"}
            </button>
            {custom && (
              <button className="btn btn-danger btn-sm" onClick={() => v.on.onRemoveCustom(p.id)} disabled={anyBusy}>
                Remove endpoint
              </button>
            )}
          </span>
          {busyOp === "connect" && <span className="mono" style={small}>Testing the key with {p.host}. It is stored only if the test passes.</span>}
        </div>
      )}

      {notice && (
        <span className="mono" data-testid="provider-notice" style={{ fontSize: 11, color: "var(--ok)" }}>
          {notice}
        </span>
      )}
      {err && (
        <span className="mono" role="alert" data-testid="provider-error" style={{ fontSize: 11.5, color: "var(--danger)" }}>
          {err}
        </span>
      )}

      {st !== "unavailable" && (p.keyConfigured || priced.length > 0) && (
        <div style={{ display: "flex", flexDirection: "column", gap: 4 }}>
          <span className="eyebrow">Models and prices</span>
          {priced.length === 0 ? (
            <span className="mono" style={{ fontSize: 11 }}>
              No models listed yet. A key test lists them.
            </span>
          ) : (
            priced.map((m) => (
              <span key={m.id} style={{ ...row, gap: 6 }}>
                <span className="mono" style={{ fontSize: 11, color: m.price ? "var(--tx-1)" : "var(--tx-3)" }}>
                  {priceText(m)}
                  {p.approvedModels.includes(m.id) ? " · approved by you" : ""}
                </span>
                {!pe && (
                  <button className="btn btn-ghost btn-sm" data-testid={`enter-price-${m.id}`} onClick={() => v.on.onEditPrice(p.id, m.id)} disabled={anyBusy || !v.desktop}>
                    {m.source === "member" ? "Change price" : m.source === "published" ? "Override price" : "Enter price"}
                  </button>
                )}
              </span>
            ))
          )}
          {unpriced > 0 && moneyCap && (
            <span data-testid="unpriced-warning" style={{ fontSize: 12, color: "var(--warn)" }}>
              {unpriced === 1 ? "1 model has no price. While a spending cap is set, calls to it are refused" : `${unpriced} models have no price. While a spending cap is set, calls to them are refused`}, because the cost would be unknown. Enter the price from {p.name}'s pricing page.
            </span>
          )}
          <span className="mono" data-testid="model-choice-note" style={small}>
            Choosing which of these models Hermes uses is not available yet. A call for a model not on this list pauses the provider and asks you.
          </span>
          <span className="mono" style={small}>
            A price you enter is used instead of a published one. Your vendor's bill is what counts.
          </span>
          {pe ? (
            <span style={row}>
              <select className="input mono" aria-label="model to price" value={pe.model} onChange={set("model")} style={{ ...field, width: 200 }}>
                <option value="">choose a model</option>
                {priced.map((m) => (
                  <option key={m.id} value={m.id}>
                    {m.id}
                  </option>
                ))}
              </select>
              <input className="input mono" aria-label="input price" placeholder="$ in / 1M" value={pe.inUsd} onChange={set("inUsd")} style={{ ...field, width: 90 }} />
              <input className="input mono" aria-label="output price" placeholder="$ out / 1M" value={pe.outUsd} onChange={set("outUsd")} style={{ ...field, width: 90 }} />
              <button className="btn btn-secondary btn-sm" onClick={v.on.onSavePrice} disabled={anyBusy}>
                Save price
              </button>
              <button className="btn btn-ghost btn-sm" onClick={() => v.on.onEditPrice(null)}>
                Cancel
              </button>
            </span>
          ) : (
            null
          )}
        </div>
      )}
    </div>
  );
}

function UsageRow(props: { p: ProviderStatus; u: ProviderUsage | undefined; v: ModelsViewProps }) {
  const { p, u, v } = props;
  const anyBusy = v.busy !== null;
  const editing = v.capsEdit && v.capsEdit.id === p.id ? v.capsEdit.form : null;
  const set = (k: keyof CapsForm) => (e: { target: { value: string } }) => editing && v.on.onCapsForm({ ...editing, [k]: e.target.value });
  const resume = p.halt ? resumeEffect(p.halt.kind) : null;
  const cap = capReached(u, p.caps);
  return (
    <div data-testid={`usage-row-${p.id}`} style={{ display: "flex", flexDirection: "column", gap: 4, padding: "12px 18px", borderBottom: "1px solid var(--line-1)" }}>
      <span style={{ fontSize: 13, fontWeight: 500 }}>
        {p.name || p.id}
        {p.kind === "custom" && <UnverifiedBadge />}
      </span>
      {u ? (
        <>
          <span className="mono" style={{ fontSize: 11.5 }}>
            Today: {u.today.calls} call{u.today.calls === 1 ? "" : "s"} · {tokenLine(u.today.tokens, p.caps.dailyTokens)} · {spendLine(u.today, p.caps.dailyMicros)}
          </span>
          <span className="mono" style={{ fontSize: 11.5 }}>
            This month: {u.month.calls} call{u.month.calls === 1 ? "" : "s"} · {tokenLine(u.month.tokens, p.caps.monthlyTokens)} · {spendLine(u.month, p.caps.monthlyMicros)}
          </span>
          {u.unsettledCalls > 0 && (
            <span className="mono" style={{ ...small, color: "var(--warn)" }}>
              {u.unsettledCalls} call{u.unsettledCalls === 1 ? " has" : "s have"} not finished. Each one counts at its full reservation until it does.
            </span>
          )}
        </>
      ) : (
        <span className="mono" style={small}>
          usage unknown
        </span>
      )}
      {cap && (
        <span className="mono" style={{ fontSize: 11.5, color: "var(--warn)" }}>
          {cap}
        </span>
      )}
      {p.halt && resume ? (
        <div data-testid="halt-block" style={{ display: "flex", flexDirection: "column", gap: 4 }}>
          <span style={{ fontSize: 12, color: "var(--warn)" }}>
            {pausedAt(p.halt.atMs)}: {p.halt.message}.
          </span>
          <span className="mono" style={small}>
            {resume.note}
          </span>
          <span>
            <button className="btn btn-secondary btn-sm" onClick={() => v.on.onResume(p.id)} disabled={anyBusy || !v.desktop}>
              {v.busy?.id === p.id && v.busy.op === "resume" ? "Resuming" : resume.label}
            </button>
          </span>
        </div>
      ) : (
        p.keyConfigured && (
          <span>
            <button className="btn btn-ghost btn-sm" onClick={() => v.on.onHalt(p.id)} disabled={anyBusy || !v.desktop}>
              Pause this provider
            </button>
          </span>
        )
      )}
      {editing ? (
        <div style={{ display: "flex", flexDirection: "column", gap: 4 }}>
          <span style={row}>
            <input className="input mono" aria-label="daily token cap" placeholder="tokens a day" value={editing.dailyTokens} onChange={set("dailyTokens")} style={{ ...field, width: 130 }} />
            <input className="input mono" aria-label="monthly token cap" placeholder="tokens a month" value={editing.monthlyTokens} onChange={set("monthlyTokens")} style={{ ...field, width: 130 }} />
            <input className="input mono" aria-label="daily spending cap" placeholder="$ a day" value={editing.dailyUsd} onChange={set("dailyUsd")} style={{ ...field, width: 100 }} />
            <input className="input mono" aria-label="monthly spending cap" placeholder="$ a month" value={editing.monthlyUsd} onChange={set("monthlyUsd")} style={{ ...field, width: 100 }} />
            <button className="btn btn-secondary btn-sm" onClick={v.on.onSaveCaps} disabled={anyBusy}>
              Save caps
            </button>
            <button className="btn btn-ghost btn-sm" onClick={() => v.on.onEditCaps(null)}>
              Cancel
            </button>
          </span>
          <span className="mono" style={small}>
            Leave a box blank for no cap of that kind. With a spending cap set, calls to a model with no price are refused, because their cost would be unknown.
          </span>
        </div>
      ) : (
        <span>
          <button className="btn btn-ghost btn-sm" onClick={() => v.on.onEditCaps(p.id)} disabled={anyBusy || !v.desktop}>
            Edit caps
          </button>
        </span>
      )}
      {v.errors["caps:" + p.id] && (
        <span className="mono" role="alert" style={{ fontSize: 11.5, color: "var(--danger)" }}>
          {v.errors["caps:" + p.id]}
        </span>
      )}
    </div>
  );
}

export function ModelsSettingsView(v: ModelsViewProps) {
  const st = v.statuses;
  const usageFor = (id: string) => v.usage?.providers.find((x) => x.id === id);
  const vendors = st?.filter((p) => p.kind === "vendor") ?? [];
  const customs = st?.filter((p) => p.kind === "custom") ?? [];
  const nameOf = (id: string) => st?.find((p) => p.id === id)?.name || id;
  const cf = v.customForm;
  const setCf = (k: keyof CustomForm) => (e: { target: { value: string } }) => v.on.onCustomForm({ ...cf, [k]: e.target.value });
  // Usage rows: every provider with a key, a pause or any recorded use this month.
  const guardHalt = st?.find((p) => p.halt?.kind === "guardUnreadable") ?? null;
  const banner = { border: "1px solid var(--danger)", borderRadius: "var(--r-2)", padding: "12px 14px", display: "flex", flexDirection: "column", gap: 6 } as const;
  const usageRows = (st ?? []).filter((p) => p.keyConfigured || p.halt || (usageFor(p.id)?.month.calls ?? 0) > 0);

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 12 }} data-testid="models-settings">
      <div style={{ border: "1px solid var(--info)", background: "var(--info-bg, var(--warn-bg))", borderRadius: "var(--r-2)", padding: "12px 14px", fontSize: 12, lineHeight: 1.55, color: "var(--tx-2)" }}>
        Connect your own key for a cloud model vendor. The key stays in this computer's <strong>OS keychain</strong> and only the Citrate Core app uses it, to call that vendor's own address and nothing else. Every call is checked against your caps first and recorded in a signed usage log on this computer (no prompts or replies are kept). Citrate never receives your keys, and you pay the vendor directly.
        {!v.desktop && (
          <>
            {" "}
            <em data-testid="models-preview-note">This web preview has no OS keychain, so vendors can only be connected in the desktop app.</em>
          </>
        )}
      </div>

      {/* ---------- fail-closed states: every provider stops until the member acts ---------- */}
      {guardHalt && (
        <div data-testid="failclosed-guard" role="alert" style={banner}>
          <span style={{ fontSize: 12.5, fontWeight: 500, color: "var(--danger)" }}>Every provider is paused: the spend settings file could not be verified.</span>
          <span style={{ fontSize: 12, color: "var(--tx-2)", lineHeight: 1.5 }}>
            The file that holds your caps, prices, pauses and custom endpoints is unreadable, was edited, or is an older copy than the one last saved. Nothing is sent to any provider until you acknowledge. Acknowledging resets caps, prices, pauses and custom endpoints for every provider to the defaults, so check them afterwards.
          </span>
          <span>
            <button className="btn btn-secondary btn-sm" onClick={() => v.on.onResume(guardHalt.id)} disabled={v.busy !== null || !v.desktop}>
              Acknowledge and reset settings
            </button>
          </span>
        </div>
      )}
      {v.usage && !v.usage.logIntact && (
        <div data-testid="usage-log-broken" role="alert" style={banner}>
          <span style={{ fontSize: 12.5, fontWeight: 500, color: "var(--danger)" }}>Every cloud call is refused: the usage log failed its check.</span>
          <span style={{ fontSize: 12, color: "var(--tx-2)", lineHeight: 1.5 }}>
            {v.usage.logProblem ? v.usage.logProblem + ". " : ""}The log may have been cut short, removed or edited. Starting a new log keeps the old file beside it; it is never deleted. This month's spend carries into the new log, so starting one does not reset your caps.
          </span>
          <span>
            <button className="btn btn-secondary btn-sm" onClick={v.on.onResetLog} disabled={v.busy !== null || !v.desktop}>
              Start a new usage log
            </button>
          </span>
        </div>
      )}
      {v.usage?.clockProblem && (
        <div data-testid="failclosed-clock" role="alert" style={banner}>
          <span style={{ fontSize: 12.5, fontWeight: 500, color: "var(--danger)" }}>Every cloud call is refused: {v.usage.clockProblem}.</span>
          <span style={{ fontSize: 12, color: "var(--tx-2)", lineHeight: 1.5 }}>
            A clock set back would move calls out of today and this month, so core stops until the clock is right. Correct this computer's date and time, then check again.
          </span>
          <span>
            <button className="btn btn-secondary btn-sm" onClick={v.on.onRefresh} disabled={v.busy !== null}>
              Check again
            </button>
          </span>
        </div>
      )}

      {v.errors[""] && (
        <span className="mono" role="alert" style={{ fontSize: 11.5, color: "var(--danger)" }}>
          {v.errors[""]}
        </span>
      )}

      {/* ---------- vendor cards ---------- */}
      <div className="surface" style={{ display: "flex", flexDirection: "column" }}>
        <div style={{ padding: "14px 18px", borderBottom: "1px solid var(--line-1)" }}>
          <span className="eyebrow">Model vendors · your key, kept on this computer</span>
        </div>
        {st === null ? (
          <span className="mono" data-testid="models-status-missing" style={{ padding: "12px 18px", fontSize: 11, color: "var(--tx-3)" }}>
            {v.loadError ? "Provider status could not be read: " + v.loadError : v.desktop ? "loading" : "Not available in the web preview."}
          </span>
        ) : (
          vendors.map((p) => <ProviderCard key={p.id} p={p} u={usageFor(p.id)} v={v} />)
        )}
        <div data-testid="provider-card-cognition" data-state="unavailable" style={{ display: "flex", flexDirection: "column", gap: 6, padding: "14px 18px", borderBottom: "1px solid var(--line-1)" }}>
          <span style={{ display: "flex", alignItems: "baseline", gap: 4 }}>
            <span style={{ fontSize: 13, fontWeight: 500 }}>Cognition · Delegate to Devin</span>
            <span className="mono" style={{ fontSize: 9.5, color: "var(--tx-3)", marginLeft: 8 }}>
              not available yet
            </span>
          </span>
          <span style={{ fontSize: 12, color: "var(--tx-2)" }}>
            Handing a task to a Devin session is not available in this build. Devin runs a remote agent rather than a chat model, so each hand-off will need your approval for that task.
          </span>
        </div>
      </div>

      {/* ---------- custom endpoints (Advanced) ---------- */}
      <div className="surface" style={{ display: "flex", flexDirection: "column" }}>
        <div style={{ padding: "14px 18px", borderBottom: "1px solid var(--line-1)", display: "flex", alignItems: "center", gap: 10 }}>
          <span className="eyebrow" style={{ flex: 1 }}>
            Custom endpoints · Unverified
          </span>
          <label style={{ fontSize: 12, display: "flex", gap: 6, alignItems: "center" }}>
            <input type="checkbox" aria-label="show advanced" checked={v.advanced} onChange={(e) => v.on.onAdvanced(e.target.checked)} />
            Advanced
          </label>
        </div>
        {customs.map((p) => (
          <ProviderCard key={p.id} p={p} u={usageFor(p.id)} v={v} />
        ))}
        {v.advanced ? (
          <div data-testid="custom-advanced" style={{ display: "flex", flexDirection: "column", gap: 8, padding: "12px 18px" }}>
            <span data-testid="custom-warning" style={{ fontSize: 12, color: "var(--warn)", lineHeight: 1.5 }}>
              A custom endpoint is not one of the governed vendors. Citrate Core cannot check who runs it or what it does with your prompts and your key, so it is labelled Unverified everywhere. It must use https, and calls are refused until you confirm its exact URL.
            </span>
            <span style={row}>
              <input className="input" aria-label="endpoint name" placeholder="name" value={cf.label} onChange={setCf("label")} style={{ ...field, width: 120 }} disabled={!v.desktop} />
              <input className="input mono" aria-label="endpoint URL" placeholder="https://…/v1" value={cf.baseUrl} onChange={setCf("baseUrl")} style={{ ...field, width: 220 }} disabled={!v.desktop} />
              <input className="input mono" aria-label="models" placeholder="models, comma separated" value={cf.models} onChange={setCf("models")} style={{ ...field, width: 200 }} disabled={!v.desktop} />
              <button className="btn btn-secondary btn-sm" onClick={v.on.onAddCustom} disabled={!v.desktop || v.busy !== null}>
                Add unverified endpoint
              </button>
            </span>
            {v.errors["custom"] && (
              <span className="mono" role="alert" style={{ fontSize: 11.5, color: "var(--danger)" }}>
                {v.errors["custom"]}
              </span>
            )}
          </div>
        ) : (
          customs.length === 0 && (
            <span className="mono" style={{ padding: "12px 18px", ...small }}>
              None. Turn on Advanced to add one.
            </span>
          )
        )}
      </div>

      {/* ---------- spend and usage (P-10) ---------- */}
      <div className="surface" style={{ display: "flex", flexDirection: "column" }} data-testid="spend-usage">
        <div style={{ padding: "14px 18px", borderBottom: "1px solid var(--line-1)", display: "flex", flexDirection: "column", gap: 3 }}>
          <span className="eyebrow">Spend and usage · from the signed usage log on this computer</span>
          {v.usage && (
            <span className="mono" style={small}>
              {v.usage.logIntact ? `Log verified · ${v.usage.records} record${v.usage.records === 1 ? "" : "s"} · as of ${fmtWhen(v.usage.asOfMs)}` : ""}
              {v.usage.logIntact ? ". Spend counts priced calls only; your vendor's bill is what counts." : ""}
            </span>
          )}
        </div>
        {v.usage === null ? (
          <span className="mono" data-testid="usage-missing" style={{ padding: "12px 18px", ...small }}>
            {v.loadError ? "Usage could not be read: " + v.loadError : v.desktop ? "loading" : "Not available in the web preview."}
          </span>
        ) : (
          <>
            {usageRows.length === 0 ? (
              <span className="mono" style={{ padding: "12px 18px", ...small }}>
                No provider is connected, so nothing has been used.
              </span>
            ) : (
              usageRows.map((p) => <UsageRow key={p.id} p={p} u={usageFor(p.id)} v={v} />)
            )}
            <div style={{ display: "flex", flexDirection: "column", gap: 4, padding: "12px 18px" }} data-testid="recent-calls">
              <span className="eyebrow">Recent calls · metadata only, no prompts or replies</span>
              {(v.usage.recentCalls ?? []).length === 0 ? (
                <span className="mono" style={small}>
                  No calls recorded yet.
                </span>
              ) : (
                (v.usage.recentCalls ?? []).map((c, i) => (
                  <span key={i} className="mono" style={{ fontSize: 11 }}>
                    {fmtWhen(c.atMs)} · {nameOf(c.provider)}
                    {c.verified ? "" : " (Unverified)"} · {c.model || "no model"} ·{" "}
                    {c.tokensIn !== null && c.tokensOut !== null ? `${fmtTokens(c.tokensIn)} in / ${fmtTokens(c.tokensOut)} out` : "tokens not reported"} ·{" "}
                    {c.costMicros !== null ? formatMicros(c.costMicros) : "cost unknown"} · {outcomeText(c, nameOf(c.provider))}
                  </span>
                ))
              )}
            </div>
          </>
        )}
      </div>
    </div>
  );
}

const message = (e: unknown): string => (e instanceof Error ? e.message : String(e));

export function ModelsSettings({ onOpen }: { onOpen(url: string): void }) {
  const desktop = BRIDGE_MODE === "tauri";
  const [statuses, setStatuses] = useState<ProviderStatus[] | null>(null);
  const [usage, setUsage] = useState<ProviderUsageSummary | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [busy, setBusy] = useState<{ id: string; op: BusyOp } | null>(null);
  const [errors, setErrors] = useState<Record<string, string>>({});
  const [notices, setNotices] = useState<Record<string, string>>({});
  const [keyDrafts, setKeyDrafts] = useState<Record<string, string>>({});
  const [confirmDrafts, setConfirmDrafts] = useState<Record<string, string>>({});
  const [capsEdit, setCapsEdit] = useState<{ id: string; form: CapsForm } | null>(null);
  const [priceEdit, setPriceEdit] = useState<{ id: string; form: PriceForm } | null>(null);
  const [advanced, setAdvanced] = useState(false);
  const [customForm, setCustomForm] = useState<CustomForm>(EMPTY_CUSTOM);

  const refresh = async () => {
    const pr = bridge.providers;
    try {
      const [s, u] = await Promise.all([pr.status(), pr.usageSummary()]);
      setStatuses(s);
      setUsage(u);
      setLoadError(null);
    } catch (e) {
      setLoadError(message(e));
    }
  };
  useEffect(() => {
    if (desktop) void refresh();
  }, [desktop]);

  const setErr = (k: string, m: string | null) =>
    setErrors((prev) => {
      const next = { ...prev };
      if (m === null) delete next[k];
      else next[k] = m;
      return next;
    });
  const setNotice = (k: string, m: string | null) =>
    setNotices((prev) => {
      const next = { ...prev };
      if (m === null) delete next[k];
      else next[k] = m;
      return next;
    });

  const act = async (id: string, op: BusyOp, errKey: string, f: () => Promise<unknown>) => {
    setBusy({ id, op });
    setErr(errKey, null);
    try {
      await f();
    } catch (e) {
      setErr(errKey, message(e));
    } finally {
      setBusy(null);
      await refresh();
    }
  };
  const find = (id: string) => statuses?.find((p) => p.id === id);
  const listed = (n: number) => (n === 1 ? "1 model listed" : `${n} models listed`);

  return (
    <ModelsSettingsView
      desktop={desktop}
      statuses={statuses}
      usage={usage}
      loadError={loadError}
      nowMs={Date.now()}
      busy={busy}
      errors={errors}
      notices={notices}
      keyDrafts={keyDrafts}
      confirmDrafts={confirmDrafts}
      capsEdit={capsEdit}
      priceEdit={priceEdit}
      advanced={advanced}
      customForm={customForm}
      on={{
        onKeyDraft: (id, val) => setKeyDrafts((d) => ({ ...d, [id]: val })),
        onConnect: (id) => {
          const key = (keyDrafts[id] ?? "").trim();
          // The draft is dropped before the call: the key lives only in the input until it goes to core.
          setKeyDrafts((d) => ({ ...d, [id]: "" }));
          setNotice(id, null);
          void act(id, "connect", id, async () => {
            try {
              const t = await bridge.providers.setKey(id, key);
              setNotice(id, `Connected. The key passed a live test (${listed(t.listedModels)}).`);
            } catch (e) {
              throw new Error(`The key was not stored: ${message(e)}`);
            }
          });
        },
        onTest: (id) => {
          setNotice(id, null);
          void act(id, "test", id, async () => {
            try {
              const t = await bridge.providers.testKey(id);
              setNotice(id, `The key works (${listed(t.listedModels)}).`);
            } catch (e) {
              throw new Error(`The key test failed: ${message(e)}`);
            }
          });
        },
        onRemove: (id) => {
          setNotice(id, null);
          void act(id, "remove", id, () => bridge.providers.removeKey(id));
        },
        onRemoveAndRevoke: (id) => {
          const page = find(id)?.revokePage ?? null;
          setNotice(id, null);
          void act(id, "revoke", id, async () => {
            await bridge.providers.removeKey(id);
            if (page) onOpen(page);
          });
        },
        onOpen,
        onHalt: (id) => void act(id, "halt", id, () => bridge.providers.halt(id)),
        onResume: (id) => void act(id, "resume", id, () => bridge.providers.resume(id)),
        onEditCaps: (id) => {
          const p = id ? find(id) : undefined;
          setCapsEdit(p ? { id: p.id, form: capsToForm(p.caps) } : null);
        },
        onCapsForm: (form) => setCapsEdit((c) => (c ? { ...c, form } : c)),
        onSaveCaps: () => {
          if (!capsEdit) return;
          const r = formToCaps(capsEdit.form);
          if ("error" in r) {
            setErr("caps:" + capsEdit.id, r.error);
            return;
          }
          const id = capsEdit.id;
          void act(id, "caps", "caps:" + id, async () => {
            await bridge.providers.setCaps(id, r.caps);
            setCapsEdit(null);
          });
        },
        onEditPrice: (id, model) => setPriceEdit(id ? { id, form: { model: model ?? "", inUsd: "", outUsd: "" } } : null),
        onPriceForm: (form) => setPriceEdit((c) => (c ? { ...c, form } : c)),
        onSavePrice: () => {
          if (!priceEdit) return;
          const { id, form } = priceEdit;
          const i = parseUsdToMicros(form.inUsd);
          const o = parseUsdToMicros(form.outUsd);
          if (!form.model || i === null || o === null) {
            setErr(id, "Choose a model and enter both prices in dollars per million tokens, from the vendor's pricing page.");
            return;
          }
          void act(id, "price", id, async () => {
            await bridge.providers.setPrice(id, form.model, { inputMicrosPerMtok: i, outputMicrosPerMtok: o });
            setPriceEdit(null);
          });
        },
        onConfirmDraft: (id, val) => setConfirmDrafts((d) => ({ ...d, [id]: val })),
        onConfirm: (id) => {
          const typed = confirmDrafts[id] ?? "";
          void act(id, "confirm", id, async () => {
            await bridge.providers.confirmCustom(id, typed);
            setConfirmDrafts((d) => ({ ...d, [id]: "" }));
          });
        },
        onRemoveCustom: (id) => void act(id, "customRemove", id, () => bridge.providers.removeCustom(id)),
        onAdvanced: setAdvanced,
        onCustomForm: setCustomForm,
        onAddCustom: () => {
          const r = customToInput(customForm);
          if ("error" in r) {
            setErr("custom", r.error);
            return;
          }
          void act("custom", "custom", "custom", async () => {
            await bridge.providers.addCustom(r.label, r.baseUrl, r.models);
            setCustomForm(EMPTY_CUSTOM);
          });
        },
        onResetLog: () => void act("", "reset", "", () => bridge.providers.resetUsageLog()),
        onRefresh: () => void refresh(),
      }}
    />
  );
}
