// =====================================================================
// HUP-S1.5 — Settings › Escalation (US-1.5 AC1, AC2).
//
// The member's own OpenAI-compatible endpoints that Hermes may escalate a hard planning step to,
// and the daily spend budget those escalations are charged to. Every value here comes from core
// (Rust `escalation.rs`): the endpoint list (never a key), today's budget and recent escalations,
// and the registry route's status (disabled, with what is missing). The key typed here goes to
// core once and is sealed in the OS keyring; it is never shown again.
//
// The view is a pure function of its props (render-tested); the container loads and refreshes.
// =====================================================================
import { useEffect, useState } from "react";
import { bridge } from "../bridge";
import { BRIDGE_MODE } from "../bridge/mode";
import type { EscalationBudget, EscalationEndpoint, EscalationEndpointInput, EscalationRegistryStatus } from "../bridge/domains";
import { formatMicros, parseUsdToMicros } from "../agent/escalation";

export interface EscalationForm {
  label: string;
  baseUrl: string;
  model: string;
  inputUsd: string;
  outputUsd: string;
  apiKey: string;
}

export const EMPTY_FORM: EscalationForm = { label: "", baseUrl: "", model: "", inputUsd: "", outputUsd: "", apiKey: "" };

/** Turn the form into the command input, or an error a person can act on. */
export function formToInput(f: EscalationForm): { input: EscalationEndpointInput; apiKey: string } | { error: string } {
  const inputMicros = parseUsdToMicros(f.inputUsd);
  const outputMicros = parseUsdToMicros(f.outputUsd);
  if (!f.label.trim()) return { error: "Give the endpoint a name." };
  if (!/^https:\/\//.test(f.baseUrl.trim()) && !/^http:\/\/(127\.0\.0\.1|localhost|\[::1\])(:\d+)?(\/|$)/.test(f.baseUrl.trim())) {
    return { error: "The endpoint must start with https:// (or http:// on this computer)." };
  }
  if (!f.model.trim()) return { error: "Enter the model name the endpoint expects." };
  if (inputMicros === null || outputMicros === null) return { error: "Enter both prices in dollars per million tokens, for example 3 or 0.15." };
  if (!f.apiKey.trim()) return { error: "Paste the endpoint's API key." };
  return {
    input: { label: f.label.trim(), baseUrl: f.baseUrl.trim(), model: f.model.trim(), inputMicrosPerMtok: inputMicros, outputMicrosPerMtok: outputMicros },
    apiKey: f.apiKey.trim(),
  };
}

function when(ms: number): string {
  if (!ms) return "";
  return new Date(ms).toISOString().slice(0, 16).replace("T", " ") + " UTC";
}

export interface EscalationViewProps {
  desktop: boolean;
  endpoints: EscalationEndpoint[] | null;
  budget: EscalationBudget | null;
  registry: EscalationRegistryStatus | null;
  form: EscalationForm;
  capInput: string;
  busy: boolean;
  error: string | null;
  onForm(f: EscalationForm): void;
  onAdd(): void;
  onRemove(id: string): void;
  onCapInput(v: string): void;
  onSetCap(): void;
}

export function EscalationSettingsView(p: EscalationViewProps) {
  const b = p.budget;
  const set = (k: keyof EscalationForm) => (e: { target: { value: string } }) => p.onForm({ ...p.form, [k]: e.target.value });
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 12 }} data-testid="escalation-settings">
      <div style={{ border: "1px solid var(--info)", background: "var(--info-bg, var(--warn-bg))", borderRadius: "var(--r-2)", padding: "12px 14px", fontSize: 12, lineHeight: 1.55, color: "var(--tx-2)" }}>
        Hermes can hand a hard planning step to a larger model on <strong>your own endpoint</strong>. Before anything is sent you see where it goes and the most it can cost. Escalations within your <strong>daily budget</strong> run with a notice; anything over it, or any task that has read untrusted content, waits for your approval. Keys are sealed in the <strong>OS keyring</strong> and handed to the agent for one request at a time.
        {!p.desktop && (
          <>
            {" "}
            <em>The web preview has no OS keyring, so endpoints can only be added in the desktop app.</em>
          </>
        )}
      </div>

      {/* ---------- daily budget ---------- */}
      <div className="surface" style={{ padding: 18, display: "flex", flexDirection: "column", gap: 10 }}>
        <span className="eyebrow">Daily escalation budget</span>
        {b ? (
          <>
            <span className="mono" style={{ fontSize: 12 }} data-testid="escalation-budget-line">
              {formatMicros(b.usedMicros)} used of {formatMicros(b.capMicros)} today · {formatMicros(b.remainingMicros)} left
              {b.confirmedMicros > 0 ? " · " + formatMicros(b.confirmedMicros) + " more approved by you one at a time" : ""}
            </span>
            {b.capMicros === 0 && (
              <span className="mono" style={{ fontSize: 10.5, color: "var(--tx-3)" }}>
                The budget is $0.00, so every escalation asks you first. This default is pending owner sign-off.
              </span>
            )}
            {b.unreadable && (
              <span className="mono" style={{ fontSize: 10.5, color: "var(--warn)" }}>
                The spend ledger could not be read, so every escalation asks. Setting a budget starts a fresh ledger.
              </span>
            )}
            <span className="mono" style={{ fontSize: 10.5, color: "var(--tx-3)" }}>
              Resets at 00:00 UTC ({when(b.periodEndMs)}). It cannot be lowered below what today already used. Most you can set: {formatMicros(b.maxCapMicros)} (pending owner sign-off).
            </span>
          </>
        ) : (
          <span className="mono" style={{ fontSize: 11, color: "var(--tx-3)" }}>budget unavailable</span>
        )}
        <span style={{ display: "flex", gap: 8, alignItems: "center" }}>
          <input className="input mono" aria-label="daily budget in dollars" placeholder="$ per day, e.g. 0.50" value={p.capInput} onChange={(e) => p.onCapInput(e.target.value)} style={{ width: 160, height: 30, fontSize: 12 }} disabled={!p.desktop || p.busy} />
          <button className="btn btn-secondary btn-sm" onClick={p.onSetCap} disabled={!p.desktop || p.busy}>
            Set budget
          </button>
        </span>
      </div>

      {/* ---------- endpoints ---------- */}
      <div className="surface" style={{ display: "flex", flexDirection: "column" }}>
        <div style={{ padding: "14px 18px", borderBottom: "1px solid var(--line-1)" }}>
          <span className="eyebrow">Your escalation endpoints · OpenAI-compatible, key sealed in the OS keyring</span>
        </div>
        {p.endpoints === null ? (
          <span className="mono" style={{ padding: "12px 18px", fontSize: 11, color: "var(--tx-3)" }}>loading</span>
        ) : p.endpoints.length === 0 ? (
          <span className="mono" style={{ padding: "12px 18px", fontSize: 11, color: "var(--tx-3)" }} data-testid="escalation-none">
            No endpoints yet. Hermes does not offer escalation until you add one.
          </span>
        ) : (
          p.endpoints.map((e) => (
            <div key={e.id} style={{ display: "flex", alignItems: "center", gap: 14, padding: "12px 18px", borderBottom: "1px solid var(--line-1)", flexWrap: "wrap" }}>
              <span style={{ flex: 1, minWidth: 160 }}>
                <span style={{ display: "block", fontSize: 13, fontWeight: 500 }}>{e.destination}</span>
                <span className="mono" style={{ display: "block", fontSize: 10.5, color: "var(--tx-3)", marginTop: 1 }}>
                  {e.model} · {formatMicros(e.inputMicrosPerMtok)} in / {formatMicros(e.outputMicrosPerMtok)} out per 1M tokens · key sealed
                </span>
              </span>
              <button className="btn btn-danger btn-sm" onClick={() => p.onRemove(e.id)} disabled={p.busy}>
                Remove
              </button>
            </div>
          ))
        )}
        <div style={{ display: "flex", gap: 8, padding: "12px 18px", flexWrap: "wrap", alignItems: "center" }}>
          <input className="input" aria-label="endpoint name" placeholder="name" value={p.form.label} onChange={set("label")} style={{ width: 120, height: 30, fontSize: 12 }} disabled={!p.desktop} />
          <input className="input mono" aria-label="endpoint URL" placeholder="https://…/v1" value={p.form.baseUrl} onChange={set("baseUrl")} style={{ width: 200, height: 30, fontSize: 12 }} disabled={!p.desktop} />
          <input className="input mono" aria-label="model" placeholder="model" value={p.form.model} onChange={set("model")} style={{ width: 130, height: 30, fontSize: 12 }} disabled={!p.desktop} />
          <input className="input mono" aria-label="input price" placeholder="$ in / 1M" value={p.form.inputUsd} onChange={set("inputUsd")} style={{ width: 90, height: 30, fontSize: 12 }} disabled={!p.desktop} />
          <input className="input mono" aria-label="output price" placeholder="$ out / 1M" value={p.form.outputUsd} onChange={set("outputUsd")} style={{ width: 90, height: 30, fontSize: 12 }} disabled={!p.desktop} />
          <input className="input mono" aria-label="API key" type="password" placeholder="API key" value={p.form.apiKey} onChange={set("apiKey")} style={{ width: 160, height: 30, fontSize: 12 }} disabled={!p.desktop} />
          <button className="btn btn-secondary btn-sm" onClick={p.onAdd} disabled={!p.desktop || p.busy}>
            Add endpoint
          </button>
        </div>
        <span className="mono" style={{ padding: "0 18px 12px", fontSize: 10.5, color: "var(--tx-3)" }}>
          Prices come from your provider's pricing page. The app cannot check them, and your provider's own bill is what counts.
        </span>
      </div>

      {p.error && (
        <span className="mono" role="alert" style={{ fontSize: 11.5, color: "var(--danger)" }}>
          {p.error}
        </span>
      )}

      {/* ---------- recent escalations ---------- */}
      {b && b.history.length > 0 && (
        <div className="surface" style={{ padding: 18, display: "flex", flexDirection: "column", gap: 6 }}>
          <span className="eyebrow">Recent escalations</span>
          {b.history.map((h) => (
            <span key={h.escalationId} className="mono" style={{ fontSize: 11 }}>
              {when(h.atMs)} · {h.destination} · {formatMicros(h.chargedMicros)} of up to {formatMicros(h.quotedMicros)} · {h.mode === "budget" ? "budget" : "you approved"} · {h.outcome.replace("_", " ")}
            </span>
          ))}
        </div>
      )}

      {/* ---------- registry route ---------- */}
      <div className="surface" style={{ padding: 18, display: "flex", flexDirection: "column", gap: 6 }} data-testid="escalation-registry">
        <span className="eyebrow">Registry models (InferenceRouter, paid in SALT) · {p.registry?.enabled ? "on" : "not available yet"}</span>
        <span style={{ fontSize: 12, color: "var(--tx-2)" }}>{p.registry?.reason ?? "status unavailable"}</span>
        {p.registry && p.registry.missing.length > 0 && (
          <ul style={{ margin: 0, paddingLeft: 18, fontSize: 11, color: "var(--tx-3)" }}>
            {p.registry.missing.map((m) => (
              <li key={m}>{m}</li>
            ))}
          </ul>
        )}
      </div>
    </div>
  );
}

const message = (e: unknown): string => (e instanceof Error ? e.message : String(e));

export function EscalationSettings() {
  const desktop = BRIDGE_MODE === "tauri";
  const [endpoints, setEndpoints] = useState<EscalationEndpoint[] | null>(null);
  const [budget, setBudget] = useState<EscalationBudget | null>(null);
  const [registry, setRegistry] = useState<EscalationRegistryStatus | null>(null);
  const [form, setForm] = useState<EscalationForm>(EMPTY_FORM);
  const [capInput, setCapInput] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refresh = async () => {
    const e = bridge.escalation;
    const [eps, bud, reg] = await Promise.all([e.endpoints().catch(() => []), e.budget().catch(() => null), e.registryStatus().catch(() => null)]);
    setEndpoints(eps);
    setBudget(bud);
    setRegistry(reg);
  };
  useEffect(() => {
    refresh();
  }, []);

  const act = async (f: () => Promise<unknown>) => {
    setBusy(true);
    setError(null);
    try {
      await f();
      await refresh();
    } catch (e) {
      setError(message(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <EscalationSettingsView
      desktop={desktop}
      endpoints={endpoints}
      budget={budget}
      registry={registry}
      form={form}
      capInput={capInput}
      busy={busy}
      error={error}
      onForm={setForm}
      onAdd={() => {
        const r = formToInput(form);
        if ("error" in r) {
          setError(r.error);
          return;
        }
        act(async () => {
          await bridge.escalation.addEndpoint(r.input, r.apiKey);
          setForm(EMPTY_FORM);
        });
      }}
      onRemove={(id) => act(() => bridge.escalation.removeEndpoint(id))}
      onCapInput={setCapInput}
      onSetCap={() => {
        const micros = parseUsdToMicros(capInput);
        if (micros === null) {
          setError("Enter the daily budget in dollars, for example 0.50.");
          return;
        }
        act(async () => {
          await bridge.escalation.setBudget(micros);
          setCapInput("");
        });
      }}
    />
  );
}
