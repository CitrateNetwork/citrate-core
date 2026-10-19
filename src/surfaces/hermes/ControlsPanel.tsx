// =====================================================================
// K2 (0.5.2): Hermes › Controls, first depth.
//
// Six switches anyone understands. Each one reads and writes the config its existing card reads and
// writes today, through the same call, so this panel adds no store of its own (Rule 7: every
// control names its data source in CONTROL_ROWS). The panel never turns anything on by itself:
// mounting only reads. A switch with no backing setting is shown disabled with the reason.
//
//   Local model only        no setting exists yet (disabled); spend line read from escalation_budget
//   Web search              hermes_web_settings_get / _set (Settings › Web search & decisions)
//   Terminal access         store.setHermesTerminal → hermes_terminal_set (Settings › App)
//   Daemons running         daemons_list / daemons_set_all_paused (Hermes home › Daemons, "Pause all")
//   Learn from my sessions  trajectories_settings_get / _set (Train › federated rounds › Training data)
//   Voice                   store.setHermesReadAloud, AppState.hermesReadAloud (Settings › App › persona)
//
// Depth 2 (providers, escalation and spend caps, MCP servers, budgets, personas) opens from each
// row's "more"; until it lands the affordance is inert and says so.
// =====================================================================
import { useEffect, useState } from "react";
import { BRIDGE_MODE } from "../../bridge/mode";
import { invoke } from "../../bridge/tauri/invoke";
import { bridge } from "../../bridge";
import type { EscalationBudget, EscalationDomain, FlRoundsDomain, FlTrajectoryStatus } from "../../bridge/domains";
import { loadWebSettings, saveWebSettings, type HermesWebStatus, type WebSettingsIo } from "../../agent/webSearch";
import { formatMicros } from "../../agent/escalation";
import { daemonsSlice } from "../../daemons/slice";
import { appDaemonActions, type DaemonActions } from "../../daemons/DaemonsPanel";
import type { Store } from "../../shell/store";
import type { AppState } from "../../shell/state";

export type ControlId = "local-model" | "web-search" | "terminal" | "daemons" | "learn" | "voice";

export interface ControlRow {
  id: ControlId;
  label: string;
  /** One plain line under the switch. No em-dashes (house style). */
  description: string;
  /** Rule 7: where the switch's value is read from and written to. */
  source: string;
}

export const CONTROL_ROWS: readonly ControlRow[] = [
  {
    id: "local-model",
    label: "Local model only, or ask before using the gateway",
    description: "Keep chat on the model on this computer, and ask before anything goes to the Citrate gateway.",
    source: "none yet (no stored setting); spend line: escalation_budget (read only)",
  },
  {
    id: "web-search",
    label: "Web search",
    description: "Let Hermes search the web and read pages. What it reads is untrusted, so later actions ask you.",
    source: "hermes_web_settings_get / hermes_web_settings_set (searchEnabled)",
  },
  {
    id: "terminal",
    label: "Terminal access",
    description: "Let Hermes run commands in folders you share. Each command asks you first and runs in a sandbox.",
    source: "AppState.hermesTerminal via store.setHermesTerminal / hermes_terminal_set",
  },
  {
    id: "daemons",
    label: "Daemons running",
    description: "Scheduled Hermes tasks you created. Turn this off to pause all of them at once.",
    source: "daemons_list / daemons_set_all_paused",
  },
  {
    id: "learn",
    label: "Learn from my sessions",
    description: "Keep verified, redacted conversations on this device for training rounds you choose to join.",
    source: "trajectories_settings_get / trajectories_settings_set",
  },
  {
    id: "voice",
    label: "Voice",
    description: "Read Hermes's replies aloud with the system voice on this computer.",
    source: "AppState.hermesReadAloud via store.setHermesReadAloud",
  },
];

/** The reads and writes the panel uses, injected so tests run without Tauri. */
export interface ControlsIo {
  web: WebSettingsIo;
  fl: Pick<FlRoundsDomain, "trajectoryStatus" | "setTrajectoryConsent">;
  daemons: Pick<DaemonActions, "available" | "refresh" | "setAllPaused">;
  escalation: Pick<EscalationDomain, "budget">;
}

/** The app's real calls: the same ones the Settings, Train and Daemons cards use. */
export function appControlsIo(): ControlsIo {
  const daemons = appDaemonActions();
  return {
    web: { mode: BRIDGE_MODE === "tauri" ? "tauri" : "sim", invoke: <T,>(cmd: string, args?: Record<string, unknown>) => invoke<T>(cmd, args) },
    fl: bridge.flRounds,
    daemons: { available: daemons.available, refresh: daemons.refresh, setAllPaused: daemons.setAllPaused },
    escalation: bridge.escalation,
  };
}

export type ControlsStore = Pick<Store, "setHermesTerminal" | "setHermesReadAloud">;
export type ControlsState = Pick<AppState, "hermesTerminal" | "hermesReadAloud">;

const msg = (e: unknown) => (e instanceof Error ? e.message : String(e));
const desc = { fontSize: 11.5, color: "var(--tx-3)", lineHeight: 1.45 } as const;
const note = { fontSize: 10.5, color: "var(--tx-3)", lineHeight: 1.45 } as const;

function Row({
  row,
  checked,
  disabled,
  onChange,
  reason,
  extra,
}: {
  row: ControlRow;
  checked: boolean;
  disabled: boolean;
  onChange?: (on: boolean) => void;
  reason?: string | null;
  extra?: React.ReactNode;
}) {
  return (
    <div
      data-testid={`control-${row.id}`}
      data-source={row.source}
      style={{ display: "flex", alignItems: "flex-start", gap: 12, padding: "12px 18px", borderBottom: "1px solid var(--line-1)" }}
    >
      <input
        type="checkbox"
        id={`hermes-control-${row.id}`}
        data-testid={`toggle-${row.id}`}
        checked={checked}
        disabled={disabled}
        aria-disabled={disabled}
        onChange={(e) => onChange?.(e.target.checked)}
        style={{ marginTop: 3 }}
      />
      <span style={{ flex: 1, minWidth: 0, display: "flex", flexDirection: "column", gap: 3 }}>
        <label htmlFor={`hermes-control-${row.id}`} style={{ fontSize: 13, fontWeight: 500, opacity: disabled ? 0.6 : 1 }}>
          {row.label}
        </label>
        <span style={desc}>{row.description}</span>
        {reason && (
          <span className="mono" data-testid={`reason-${row.id}`} style={note}>
            {reason}
          </span>
        )}
        {extra}
      </span>
      <button
        className="btn btn-ghost btn-sm"
        data-testid={`more-${row.id}`}
        disabled
        aria-disabled="true"
        title="More settings for this control are coming next"
        style={{ opacity: 0.5, cursor: "not-allowed", whiteSpace: "nowrap" }}
      >
        More · coming next
      </button>
    </div>
  );
}

export function ControlsPanel({ store, s, io: ioProp }: { store: ControlsStore; s: ControlsState; io?: ControlsIo }) {
  // One io per mount (the default builds the app's real calls once, not on every render).
  const [io] = useState<ControlsIo>(() => ioProp ?? appControlsIo());
  const rows = Object.fromEntries(CONTROL_ROWS.map((r) => [r.id, r])) as Record<ControlId, ControlRow>;
  const webDesktop = io.web.mode === "tauri";

  const [web, setWeb] = useState<HermesWebStatus | null>(null);
  const [webErr, setWebErr] = useState<string | null>(null);
  const [learn, setLearn] = useState<FlTrajectoryStatus | null>(null);
  const [learnErr, setLearnErr] = useState<string | null>(null);
  const [budget, setBudget] = useState<EscalationBudget | null>(null);
  const [busy, setBusy] = useState<ControlId | null>(null);
  const [daemonErr, setDaemonErr] = useState<string | null>(null);
  const daemons = daemonsSlice.use();

  // Reads only. Nothing here writes, so opening the panel changes no setting.
  useEffect(() => {
    let live = true;
    if (webDesktop) {
      loadWebSettings(io.web).then(
        (st) => live && (setWeb(st), setWebErr(st.loadError ?? null)),
        (e) => live && setWebErr(msg(e)),
      );
    }
    io.fl.trajectoryStatus().then(
      (st) => live && (setLearn(st), setLearnErr(st.loadError)),
      (e) => live && setLearnErr(msg(e)),
    );
    io.escalation.budget().then(
      (b) => live && setBudget(b),
      () => live && setBudget(null),
    );
    if (io.daemons.available) void io.daemons.refresh();
    return () => {
      live = false;
    };
  }, [io]);

  const run = async (id: ControlId, f: () => Promise<void>) => {
    setBusy(id);
    try {
      await f();
    } finally {
      setBusy(null);
    }
  };

  // ---------- web search ----------
  const webOn = web?.settings.searchEnabled === true;
  const setWebSearch = (on: boolean) =>
    run("web-search", async () => {
      if (!web) return;
      try {
        const st = await saveWebSettings(io.web, { ...web.settings, searchEnabled: on });
        setWeb(st);
        setWebErr(st.loadError ?? null);
      } catch (e) {
        setWebErr(msg(e));
      }
    });
  const webReason = !webDesktop
    ? "Web search needs the desktop app. This preview cannot change it."
    : webErr ?? (web?.searxngFound === false && webOn ? "Search is on, but no SearXNG program is set yet (More, in Settings › Web search)." : null);

  // ---------- learn from my sessions ----------
  const learnOn = learn?.settings.enabled === true;
  const setLearnOn = (on: boolean) =>
    run("learn", async () => {
      try {
        // Turning it off keeps recorded sessions; deleting them is a second-depth choice.
        const st = await io.fl.setTrajectoryConsent(on, false);
        setLearn(st);
        setLearnErr(st.loadError);
      } catch (e) {
        setLearnErr(msg(e));
      }
    });

  // ---------- daemons ----------
  const total = daemons.view?.daemons.length ?? 0;
  const allPaused = daemons.view?.allPaused ?? false;
  const daemonsOn = total > 0 && !allPaused;
  const daemonsDisabled = !io.daemons.available || !daemons.view || total === 0 || busy === "daemons";
  const daemonReason = !io.daemons.available
    ? "Daemons run in the desktop app."
    : daemonErr ?? daemons.error ?? (daemons.view && total === 0 ? "No daemons yet. Nothing runs until you create one." : daemons.view ? `${total} daemon${total === 1 ? "" : "s"}${allPaused ? ", all paused" : ""}.` : null);
  const setDaemonsOn = (on: boolean) =>
    run("daemons", async () => {
      setDaemonErr(await io.daemons.setAllPaused(!on));
    });

  return (
    <div className="surface" data-testid="hermes-controls" style={{ display: "flex", flexDirection: "column" }}>
      <div style={{ padding: "14px 18px", borderBottom: "1px solid var(--line-1)", display: "flex", flexDirection: "column", gap: 3 }}>
        <span className="eyebrow">Hermes · controls</span>
        <span style={desc}>These switches change the same settings as their cards in Settings. Opening this panel changes nothing.</span>
      </div>

      <Row
        row={rows["local-model"]}
        checked={false}
        disabled
        reason="This switch is not available yet: there is no saved setting for this. Today a stopped local model already makes Hermes ask before a message goes to the gateway."
        extra={
          <span className="mono" data-testid="local-model-spend" style={note}>
            {budget
              ? `Escalation budget ${formatMicros(budget.capMicros)} a day, ${formatMicros(budget.usedMicros)} used.${budget.capMicros === 0 ? " Every paid escalation asks you first." : ""}`
              : "Escalation budget unavailable."}
          </span>
        }
      />

      <Row row={rows["web-search"]} checked={webOn} disabled={!webDesktop || web === null || busy === "web-search"} onChange={(on) => void setWebSearch(on)} reason={webReason} />

      <Row row={rows.terminal} checked={!!s.hermesTerminal} disabled={busy === "terminal"} onChange={(on) => void run("terminal", () => store.setHermesTerminal(on))} />

      <Row row={rows.daemons} checked={daemonsOn} disabled={daemonsDisabled} onChange={(on) => void setDaemonsOn(on)} reason={daemonReason} />

      <Row row={rows.learn} checked={learnOn} disabled={learn === null || busy === "learn"} onChange={(on) => void setLearnOn(on)} reason={learnErr} />

      <Row row={rows.voice} checked={!!s.hermesReadAloud} disabled={false} onChange={(on) => store.setHermesReadAloud(on)} />
    </div>
  );
}
