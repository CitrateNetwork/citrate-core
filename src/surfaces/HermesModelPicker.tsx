// =====================================================================
// D-11 (RC52, ADR P-5) — which model Hermes's agent loop runs on.
//
// The local model, or a cloud model from a provider the member connected in Settings › Models.
// A cloud model runs the same loop (the same tools, approvals and taint rules) through the core
// model proxy: the key stays in this computer's keychain, the agent only gets a one-session route,
// and the provider's caps, halt and usage log apply. Data sources (Rule 7): `providers_status`
// (connected providers, their listed and approved models) and `providers_preferred_get/set` (the
// stored choice, `<app data>/providers/preferred.json`). The view is a pure function of its props
// (render-tested); the container loads the status.
// =====================================================================
import { useEffect, useState } from "react";
import { bridge } from "../bridge";
import type { ChatRoute, ProviderStatus } from "../bridge/domains";

export type CloudModelChoice = { providerId: string; model: string } | null;

/** One selectable cloud model. */
export interface CloudModelOption {
  providerId: string;
  providerName: string;
  model: string;
  /** A custom endpoint the member added (never a governed vendor). */
  unverified: boolean;
}

/** Most models listed per provider (a vendor's list can run to hundreds). */
export const MAX_MODELS_PER_PROVIDER = 60;

/** The models Hermes can run on: providers with a stored key, an adapter in this build, no halt,
 *  and (custom endpoints) the member's confirmation. Cognition is a delegate, never a chat model. */
export function cloudModelOptions(statuses: readonly ProviderStatus[]): CloudModelOption[] {
  const out: CloudModelOption[] = [];
  for (const p of statuses) {
    if (!p.keyConfigured || !p.adapter || p.halt || p.kind === "delegate" || p.kind === "gateway") continue;
    if (p.kind === "custom" && !p.confirmed) continue;
    const models = [...new Set([...p.approvedModels, ...p.listedModels])].slice(0, MAX_MODELS_PER_PROVIDER);
    for (const model of models) out.push({ providerId: p.id, providerName: p.name, model, unverified: p.kind === "custom" });
  }
  return out;
}

const key = (c: { providerId: string; model: string }) => `${c.providerId}::${c.model}`;

export interface HermesModelPickerViewProps {
  value: CloudModelChoice;
  options: CloudModelOption[] | null;
  /** Why the options could not be read (the web preview, an unreadable status). */
  error: string | null;
  route: ChatRoute;
  onChoose(choice: CloudModelChoice): void;
}

export function HermesModelPickerView(p: HermesModelPickerViewProps) {
  const options = p.options ?? [];
  const current = p.value ? key(p.value) : "local";
  // A stored choice whose provider is no longer connected stays visible as such (never silently local).
  const missing = p.value && !options.some((o) => key(o) === current);
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
      <label style={{ display: "flex", alignItems: "center", gap: 10, fontSize: 13 }}>
        Model
        <select
          id="hermes-cloud-model"
          className="input"
          value={current}
          onChange={(e) => {
            const v = e.target.value;
            if (v === "local") return p.onChoose(null);
            const o = options.find((x) => key(x) === v);
            if (o) p.onChoose({ providerId: o.providerId, model: o.model });
          }}
          style={{ height: 30, fontSize: 12 }}
        >
          <option value="local">Local model on this computer</option>
          {missing && p.value && <option value={current}>{`${p.value.providerId} · ${p.value.model} (not connected now)`}</option>}
          {options.map((o) => (
            <option key={key(o)} value={key(o)}>
              {`${o.providerName} · ${o.model}${o.unverified ? " (custom endpoint, unverified)" : ""}`}
            </option>
          ))}
        </select>
      </label>
      <span style={{ fontSize: 11.5, color: "var(--tx-3)", lineHeight: 1.5 }}>
        A cloud model runs the same loop, tools and approvals through the core model proxy. Your key stays in this computer's keychain and is never given to the agent; the provider bills you, within the caps and usage shown in Settings › Models.
      </span>
      {p.route === "local_only" && p.value && (
        <span style={{ fontSize: 11.5, color: "var(--warn)" }}>Local model only is on, so Hermes uses the local model and nothing goes to the cloud.</span>
      )}
      {missing && (
        <span style={{ fontSize: 11.5, color: "var(--warn)" }}>That provider is not connected now (or is halted), so a new conversation will not start on it. Choose another model or reconnect it in Settings › Models.</span>
      )}
      {p.error && <span style={{ fontSize: 11.5, color: "var(--tx-3)" }}>Cloud models are not available here: {p.error}</span>}
      {!p.error && p.options && options.length === 0 && (
        <span style={{ fontSize: 11.5, color: "var(--tx-3)" }}>No cloud model is connected yet. Connect a provider in Settings › Models to choose one here.</span>
      )}
    </div>
  );
}

/** The container: reads the connected providers once; a choice is saved in core
 *  (`providers_preferred_set`), and core's refusal is shown. */
export function HermesModelPicker(props: { value: CloudModelChoice; route: ChatRoute; onChoose(choice: CloudModelChoice): Promise<string | null> }) {
  const [options, setOptions] = useState<CloudModelOption[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [refused, setRefused] = useState<string | null>(null);
  useEffect(() => {
    let live = true;
    bridge.providers
      .status()
      .then((st) => live && setOptions(cloudModelOptions(st)))
      .catch((e: unknown) => live && setError(e instanceof Error ? e.message : String(e)));
    return () => {
      live = false;
    };
  }, []);
  return (
    <>
      <HermesModelPickerView
        value={props.value}
        options={options}
        error={error}
        route={props.route}
        onChoose={(c) => {
          setRefused(null);
          void props.onChoose(c).then(setRefused);
        }}
      />
      {refused && <span style={{ fontSize: 11.5, color: "var(--warn)" }}>Not saved: {refused}</span>}
    </>
  );
}
