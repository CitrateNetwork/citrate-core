// =====================================================================
// PersonaPicker — HUP-S3.3 + S3.7 (US-3.3): choose Hermes's voice.
//
// The shipped personas, their fragments and every rule about them come from the Hermes sidecar
// (runtime agent-loop `personas` + `workflows`, served at /personas, /personas/check and
// /workflows through core's bearer-authed control client). Their names are placeholders pending
// owner sign-off, and the picker says so next to each name. "Hermes (default voice)" is the
// default and changes nothing. A persona shapes tone and wording only: it never grants a tool or
// changes an approval, a gate or the signing ceremony.
//
// Used in Settings (full) and at the end of onboarding (compact: the choice only).
// =====================================================================
import { useEffect, useMemo, useState } from "react";
import type { CustomPersonaInput, HermesPersona, TrackWorkflow } from "../bridge/domains";
import {
  DEFAULT_VOICE_LABEL,
  evidenceLabel,
  nameLabel,
  personaRefusal,
  refreshChosen,
  validateCustomPersonaInput,
  workflowsForTrack,
  type CustomPersonaForm,
} from "../agent/personas";

/** The three sidecar calls the picker needs (bridge.agentHarness implements them). */
export interface PersonaApi {
  personas(): Promise<HermesPersona[]>;
  workflows(): Promise<TrackWorkflow[]>;
  personaCheck(p: CustomPersonaInput): Promise<HermesPersona>;
}

const lbl = { fontFamily: "var(--font-mono)", fontSize: 10, letterSpacing: ".12em", textTransform: "uppercase" as const, color: "var(--tx-3)" };
const note = { fontSize: 11.5, color: "var(--tx-3)", lineHeight: 1.5 };
const EMPTY_FORM: CustomPersonaForm = { name: "", summary: "", voice: "", tone: "", rules: "", default_track: "", tts_voice: "" };

type Load = { state: "loading" } | { state: "ready" } | { state: "unavailable"; reason: string };

function PersonaOption({
  p,
  checked,
  onPick,
  onRemove,
  compact,
}: {
  p: HermesPersona | null;
  checked: boolean;
  onPick: () => void;
  onRemove?: () => void;
  compact?: boolean;
}) {
  const id = p ? p.id : "default";
  return (
    <div data-testid={`persona-option-${id}`} style={{ display: "flex", gap: 10, alignItems: "flex-start", padding: "8px 0", borderTop: "1px solid var(--line, rgba(127,127,127,.15))" }}>
      <input type="radio" name="hermes-persona" data-testid={`persona-radio-${id}`} checked={checked} onChange={onPick} style={{ marginTop: 3 }} />
      <div style={{ flex: 1, display: "flex", flexDirection: "column", gap: 3 }}>
        <span style={{ fontSize: 13, fontWeight: 500 }}>
          {p ? nameLabel(p) : DEFAULT_VOICE_LABEL}
          {p && <span style={{ ...lbl, marginLeft: 8 }}>{p.custom ? "custom" : p.role}</span>}
        </span>
        {p ? (
          <>
            <span style={note}>{p.summary}</span>
            {!compact && (
              <span style={note}>
                Voice: {p.voice} Tone: {p.tone}
                <br />
                Default track: {p.default_track}, workflow {p.default_workflow}. Speech: {p.tts_voice ? `${p.tts_voice} (stored, not used by speech yet)` : "system voice"}.
                {p.tool_emphasis.length > 0 && (
                  <>
                    <br />
                    Reaches for: {p.tool_emphasis.join(", ")}
                  </>
                )}
              </span>
            )}
          </>
        ) : (
          <span style={note}>Hermes's own voice. Nothing about the prompt changes.</span>
        )}
      </div>
      {onRemove && (
        <button className="btn btn-ghost" data-testid={`persona-remove-${id}`} onClick={onRemove}>
          Remove
        </button>
      )}
    </div>
  );
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <label style={{ display: "flex", flexDirection: "column", gap: 4 }}>
      <span style={{ fontSize: 12.5, color: "var(--tx-2)" }}>{label}</span>
      {children}
    </label>
  );
}

export function PersonaPicker({
  api,
  chosen,
  custom,
  onChoose,
  onAddCustom,
  onRemoveCustom,
  compact,
}: {
  api: PersonaApi;
  chosen: HermesPersona | null;
  custom: HermesPersona[];
  onChoose: (p: HermesPersona | null) => void;
  onAddCustom: (p: HermesPersona) => void;
  onRemoveCustom: (id: string) => void;
  compact?: boolean;
}) {
  const [shipped, setShipped] = useState<HermesPersona[]>([]);
  const [workflows, setWorkflows] = useState<TrackWorkflow[]>([]);
  const [load, setLoad] = useState<Load>({ state: "loading" });
  const [formOpen, setFormOpen] = useState(false);
  const [form, setForm] = useState<CustomPersonaForm>(EMPTY_FORM);
  const [formError, setFormError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const customList = Array.isArray(custom) ? custom : [];

  useEffect(() => {
    let live = true;
    (async () => {
      try {
        const [ps, ws] = await Promise.all([api.personas(), api.workflows()]);
        if (!live) return;
        setShipped(ps);
        setWorkflows(ws);
        setLoad({ state: "ready" });
      } catch (e) {
        if (live) setLoad({ state: "unavailable", reason: personaRefusal(e) });
      }
    })();
    return () => {
      live = false;
    };
  }, [api]);

  // A saved shipped choice picks up the sidecar's current view (a rename once the owner signs off).
  useEffect(() => {
    if (load.state !== "ready") return;
    const fresh = refreshChosen(chosen, shipped);
    if (fresh && chosen && JSON.stringify(fresh) !== JSON.stringify(chosen)) onChoose(fresh);
  }, [load.state, shipped, chosen, onChoose]);

  // The saved shipped choice stays visible even when the sidecar is down (its fragment was saved).
  const shippedShown = useMemo(() => {
    if (chosen && !chosen.custom && !shipped.some((p) => p.id === chosen.id)) return [chosen, ...shipped];
    return shipped;
  }, [chosen, shipped]);
  const trackIds = useMemo(() => [...new Set(workflows.map((w) => w.track))], [workflows]);
  const family = chosen ? workflowsForTrack(workflows, chosen.default_track) : [];

  const set = (k: keyof CustomPersonaForm) => (e: { target: { value: string } }) => setForm((f) => ({ ...f, [k]: e.target.value }));

  async function save() {
    setFormError(null);
    const r = validateCustomPersonaInput(form, customList, shipped);
    if (!r.ok) {
      setFormError(r.error);
      return;
    }
    setSaving(true);
    try {
      const view = await api.personaCheck(r.value);
      onAddCustom(view);
      setForm(EMPTY_FORM);
      setFormOpen(false);
    } catch (e) {
      setFormError(personaRefusal(e));
    } finally {
      setSaving(false);
    }
  }

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 6 }} data-testid="persona-picker">
      <span style={note}>
        A persona sets how Hermes writes: voice, tone and style rules. It does not change what Hermes may do; every approval and gate stays the same.
        {" "}The shipped names are placeholders, pending owner sign-off.
      </span>
      {load.state === "loading" && <span style={note}>Loading personas from the Hermes sidecar...</span>}
      {load.state === "unavailable" && (
        <span style={note} data-testid="persona-unavailable">
          The shipped personas load from the Hermes sidecar, which is not answering ({load.reason}). Your saved choice and custom personas still work.
        </span>
      )}
      <div>
        <PersonaOption p={null} checked={!chosen} onPick={() => onChoose(null)} compact={compact} />
        {shippedShown.map((p) => (
          <PersonaOption key={p.id} p={p} checked={chosen?.id === p.id} onPick={() => onChoose(p)} compact={compact} />
        ))}
        {customList.map((p) => (
          <PersonaOption
            key={p.id}
            p={p}
            checked={chosen?.id === p.id}
            onPick={() => onChoose(p)}
            onRemove={compact ? undefined : () => onRemoveCustom(p.id)}
            compact={compact}
          />
        ))}
      </div>

      {compact && <span style={note}>You can change this later in Settings, under App.</span>}

      {!compact && chosen && family.length > 0 && (
        <div data-testid="persona-workflows" style={{ display: "flex", flexDirection: "column", gap: 4, marginTop: 6 }}>
          <span style={lbl}>Track {chosen.default_track} · workflows</span>
          {family.map((w) => (
            <span key={w.id} style={note}>
              <span className="mono">{w.id}</span>
              {w.is_default ? " (default)" : ""}: {w.summary} ({evidenceLabel(w.evidence)})
            </span>
          ))}
          <span style={note}>Workflows are defined here; running them from chat is not available yet.</span>
        </div>
      )}

      {!compact && (
        <div style={{ marginTop: 8 }}>
          {!formOpen ? (
            <button className="btn btn-ghost" data-testid="persona-custom-open" onClick={() => setFormOpen(true)}>
              Create a custom persona
            </button>
          ) : (
            <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
              <span style={lbl}>Custom persona</span>
              <Field label="Name">
                <input className="input" data-testid="persona-custom-name" value={form.name} onChange={set("name")} />
              </Field>
              <Field label="Summary (one line)">
                <input className="input" data-testid="persona-custom-summary" value={form.summary} onChange={set("summary")} />
              </Field>
              <Field label="Voice">
                <input className="input" data-testid="persona-custom-voice" value={form.voice} onChange={set("voice")} />
              </Field>
              <Field label="Tone">
                <input className="input" data-testid="persona-custom-tone" value={form.tone} onChange={set("tone")} />
              </Field>
              <Field label="Writing-style rules (one per line)">
                <textarea className="input" rows={4} data-testid="persona-custom-rules" value={form.rules} onChange={set("rules")} />
              </Field>
              <Field label="Default track">
                <select className="input" data-testid="persona-custom-track" value={form.default_track} onChange={set("default_track")}>
                  <option value="">Pick a track</option>
                  {trackIds.map((t) => (
                    <option key={t} value={t}>
                      {t}
                    </option>
                  ))}
                </select>
              </Field>
              <Field label="Speech voice id (optional; stored for later, not used by speech yet)">
                <input className="input" data-testid="persona-custom-tts" value={form.tts_voice ?? ""} onChange={set("tts_voice")} />
              </Field>
              {formError && (
                <span data-testid="persona-custom-error" style={{ fontSize: 12, color: "var(--danger, #c0392b)" }}>
                  {formError}
                </span>
              )}
              <span style={{ display: "flex", gap: 8 }}>
                <button className="btn btn-primary" data-testid="persona-custom-save" disabled={saving} onClick={() => void save()}>
                  {saving ? "Checking..." : "Check and save"}
                </button>
                <button
                  className="btn btn-ghost"
                  onClick={() => {
                    setFormOpen(false);
                    setFormError(null);
                  }}
                >
                  Cancel
                </button>
              </span>
              <span style={note}>The Hermes sidecar checks the persona before it is saved on this device.</span>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
