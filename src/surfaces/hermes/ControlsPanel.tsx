// =====================================================================
// K2 (0.5.2): Hermes › Controls, first depth.
//
// Six switches anyone understands. Each one reads and writes the config its existing card reads and
// writes today, through the same call, so this panel adds no store of its own (Rule 7: every
// control names its data source in CONTROL_ROWS). The panel never turns anything on by itself:
// mounting only reads. A switch with no backing setting is shown disabled with the reason.
//
//   Local model only        chat_route_get / chat_route_set via store.setChatRoute (chat_route.rs:
//                           local_only or ask_first, the default); spend line read from escalation_budget
//   Web search              hermes_web_settings_get / _set (Details › Web search & decisions); each write
//                           re-reads the saved settings first and changes only searchEnabled
//   Terminal access         store.setHermesTerminal → hermes_terminal_set (Details › Terminal & folders)
//   Daemons running         daemons_list / daemons_set_all_paused (Hermes home › Daemons, "Pause all")
//   Learn from my sessions  trajectories_settings_get / _set (Train › federated rounds › Training data)
//   Voice                   store.setHermesReadAloud, AppState.hermesReadAloud (Details › Personas & voice)
//
// Saved is not effective: web search and learning are read by Hermes when it starts (core answers
// `appliesOnRestart`), so after a change the row says the running Hermes keeps its previous setting
// until it restarts, with the existing restart (the Agent page's stop + start) when one is wired.
//
// Depth 2 (providers, escalation and spend caps, MCP servers, web search, terminal and folders,
// budgets, personas) opens from each row's "Details": CONTROL_DETAILS names where each row goes.
// =====================================================================
import { useEffect, useState } from "react";
import { BRIDGE_MODE } from "../../bridge/mode";
import { invoke } from "../../bridge/tauri/invoke";
import { bridge } from "../../bridge";
import type { EscalationBudget, EscalationDomain, FlRoundsDomain, FlTrajectoryStatus } from "../../bridge/domains";
import { loadWebSettings, type HermesWebStatus, type WebSettingsIo } from "../../agent/webSearch";
import { formatMicros } from "../../agent/escalation";
import { daemonsSlice } from "../../daemons/slice";
import { restartHermesConfirmed, type HermesRestartResult } from "../../shell/slices/agent";
import { HELD_DURING_START, confirmRunning, hermesRunningSlice, hermesStarting, noteRunning, withHermesStart, type HermesSavedSnapshot } from "../../shell/slices/hermesRunning";
import { subscribeWebSettings } from "../../agent/webSettingsSync";
import { patchWebSettings, writeStartSettingUnlessStarting } from "../../agent/startSettingsWrite";
import { appDaemonActions, type DaemonActions } from "../../daemons/DaemonsPanel";
import type { Store } from "../../shell/store";
import type { AppState } from "../../shell/state";
import { readWebSettingsUnlessSuperseded } from "../../agent/webSettingsSync";

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
    description: "On: chat stays on the model on this computer and never uses the gateway. Off: Hermes asks before each message goes to the gateway.",
    source: "AppState.chatRoute via store.setChatRoute / chat_route_get / chat_route_set; spend line: escalation_budget (read only)",
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

/** Where each row's "Details" opens: Hermes › Controls › 2 · Details on a Settings section, or
 *  Hermes › Learn for learning (design slice 1). */
export const CONTROL_DETAILS: Record<ControlId, { route: string; sSec?: string }> = {
  "local-model": { route: "hermes/controls/details", sSec: "ai" },
  "web-search": { route: "hermes/controls/details", sSec: "web" },
  terminal: { route: "hermes/controls/details", sSec: "terminal" },
  daemons: { route: "hermes/controls/details", sSec: "budgets" },
  learn: { route: "hermes/learn" },
  voice: { route: "hermes/controls/details", sSec: "personas" },
};

/** The reads and writes the panel uses, injected so tests run without Tauri. */
export interface ControlsIo {
  web: WebSettingsIo;
  fl: Pick<FlRoundsDomain, "trajectoryStatus" | "setTrajectoryConsent">;
  daemons: Pick<DaemonActions, "available" | "refresh" | "setAllPaused">;
  escalation: Pick<EscalationDomain, "budget">;
  /** Restart Hermes so saved sidecar settings apply, confirmed against the harness status. Optional:
   *  without it no restart button shows. */
  restartHermes?: () => Promise<HermesRestartResult>;
  /** True in the desktop app (the settings live in core). Defaults to the web settings' mode. */
  desktop?: boolean;
}

/** The app's real calls: the same ones the Settings, Train and Daemons cards use. */
export function appControlsIo(): ControlsIo {
  const daemons = appDaemonActions();
  return {
    web: { mode: BRIDGE_MODE === "tauri" ? "tauri" : "sim", invoke: <T,>(cmd: string, args?: Record<string, unknown>) => invoke<T>(cmd, args) },
    fl: bridge.flRounds,
    daemons: { available: daemons.available, refresh: daemons.refresh, setAllPaused: daemons.setAllPaused },
    escalation: bridge.escalation,
    // The Agent page's stop + start, each step confirmed by the harness status.
    restartHermes: restartHermesConfirmed,
  };
}

export type ControlsStore = Pick<Store, "setHermesTerminal" | "setHermesReadAloud" | "setChatRoute">;
export type ControlsState = Pick<AppState, "hermesTerminal" | "hermesReadAloud"> & Partial<Pick<AppState, "chatRoute">>;

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
  pending,
  onRestart,
  restarting,
  restartError,
  onDetails,
}: {
  row: ControlRow;
  checked: boolean;
  disabled: boolean;
  onChange?: (on: boolean) => void;
  reason?: string | null;
  extra?: React.ReactNode;
  /** Saved but not yet in effect: what the running Hermes still does. */
  pending?: string | null;
  onRestart?: () => void;
  restarting?: boolean;
  /** The last restart failed: what happened, shown until a restart is confirmed. */
  restartError?: string | null;
  /** Open this control's second depth. Without it no Details button is drawn. */
  onDetails?: () => void;
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
        {pending && (
          <span data-testid={`pending-${row.id}`} style={{ ...note, color: "var(--warn)", display: "flex", gap: 8, alignItems: "center", flexWrap: "wrap" }}>
            <span>Saved. Takes effect when Hermes restarts. {pending}</span>
            {onRestart && (
              <button className="btn btn-ghost btn-sm" data-testid={`restart-${row.id}`} disabled={restarting} onClick={onRestart}>
                {restarting ? "Restarting…" : restartError ? "Retry restart" : "Restart Hermes"}
              </button>
            )}
          </span>
        )}
        {pending && restartError && (
          <span className="mono" role="alert" data-testid={`restart-error-${row.id}`} style={{ ...note, color: "var(--danger)" }}>
            {restartError}
          </span>
        )}
      </span>
      {onDetails && (
        <button className="btn btn-ghost btn-sm" data-testid={`more-${row.id}`} onClick={onDetails} style={{ whiteSpace: "nowrap", flexShrink: 0 }}>
          Details ›
        </button>
      )}
    </div>
  );
}

export function ControlsPanel({ store, s, io: ioProp, onDetails }: { store: ControlsStore; s: ControlsState; io?: ControlsIo; /** Open a row's second depth (CONTROL_DETAILS). */ onDetails?: (id: ControlId) => void }) {
  // One io per mount (the default builds the app's real calls once, not on every render).
  const [io] = useState<ControlsIo>(() => ioProp ?? appControlsIo());
  const rows = Object.fromEntries(CONTROL_ROWS.map((r) => [r.id, r])) as Record<ControlId, ControlRow>;
  const webDesktop = io.web.mode === "tauri";
  const desktop = io.desktop ?? webDesktop;

  const [web, setWeb] = useState<HermesWebStatus | null>(null);
  const [webErr, setWebErr] = useState<string | null>(null);
  const [learn, setLearn] = useState<FlTrajectoryStatus | null>(null);
  const [learnErr, setLearnErr] = useState<string | null>(null);
  const [budget, setBudget] = useState<EscalationBudget | null>(null);
  const [busy, setBusy] = useState<ControlId | null>(null);
  const [daemonErr, setDaemonErr] = useState<string | null>(null);
  const daemons = daemonsSlice.use();
  // What the running Hermes started with, as far as this panel knows: the saved value when the
  // panel opened, or after the last restart from here. Saved differs from it = pending a restart.
  // App-wide (hermesRunning.ts), so a pending restart survives leaving and reopening this panel.
  const running = hermesRunningSlice.use();
  const webStarted = running.webSearch;
  const learnStarted = running.learn;
  // A start or restart is in flight (from here, the Agent page or the terminal switch): Hermes is
  // reading the saved values, so the two start-time switches hold until it is up.
  const held = running.starting > 0;
  const [restarting, setRestarting] = useState(false);
  const [restartError, setRestartError] = useState<string | null>(null);

  // One source of truth with the Settings › Web search form mounted below: when it saves, show
  // what core saved (webSettingsSync.ts). This panel announces its own saves the same way.
  const [me] = useState(() => Symbol("controls-panel"));
  useEffect(
    () =>
      subscribeWebSettings((st, source) => {
        if (source === me) return;
        setWeb(st);
        setWebErr(st.loadError ?? null);
      }),
    [me],
  );

  // Reads only. Nothing here writes, so opening the panel changes no setting.
  useEffect(() => {
    let live = true;
    if (webDesktop) {
      // Dropped (null) if the form saved while this read was in flight: the subscription applied it.
      readWebSettingsUnlessSuperseded(() => loadWebSettings(io.web)).then(
        (st) => live && st && (setWeb(st), setWebErr(st.loadError ?? null), noteRunning("webSearch", st.settings.searchEnabled)),
        (e) => live && setWebErr(msg(e)),
      );
    }
    io.fl.trajectoryStatus().then(
      (st) => live && (setLearn(st), setLearnErr(st.loadError), noteRunning("learn", st.settings.enabled)),
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

  const det = (id: ControlId) => (onDetails ? () => onDetails(id) : undefined);

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
      if (!web || hermesStarting()) return;
      try {
        // Field-specific: re-read what is saved now (another form may have changed it since this
        // panel loaded), change only searchEnabled, and show what core answers.
        // One write at a time with the Web search form (patchWebSettings, startSettingsWrite.ts).
        // Held (null) if a Hermes start began while this write waited; the row says so.
        const st = await patchWebSettings(io.web, { searchEnabled: on }, me);
        if (!st) return;
        setWeb(st);
        setWebErr(st.loadError ?? null);
      } catch (e) {
        setWebErr(msg(e));
      }
    });
  const webReason = !webDesktop
    ? "Web search needs the desktop app. This preview cannot change it."
    : held
      ? HELD_DURING_START
      : webErr ?? (web?.searxngFound === false && webOn ? "Search is on, but no SearXNG program is set yet (set it in Hermes › Controls › Details › Web search & decisions)." : null);

  const webPending =
    web && web.appliesOnRestart && webStarted !== null && web.settings.searchEnabled !== webStarted
      ? webStarted
        ? "Until then the running Hermes still has web search on."
        : "Until then the running Hermes does not have web search."
      : null;

  // ---------- learn from my sessions ----------
  const learnOn = learn?.settings.enabled === true;
  const setLearnOn = (on: boolean) =>
    run("learn", async () => {
      if (hermesStarting()) return;
      try {
        // Turning it off keeps recorded sessions; deleting them is a second-depth choice.
        // Under the start-time settings lock: a Hermes start that begins meanwhile waits for this
        // write, and a write asked for during a start is held (null); the row says so.
        const st = await writeStartSettingUnlessStarting(async () => {
          const fresh = await io.fl.trajectoryStatus();
          noteRunning("learn", fresh.settings.enabled);
          return io.fl.setTrajectoryConsent(on, false);
        });
        if (!st) return;
        setLearn(st);
        setLearnErr(st.loadError);
      } catch (e) {
        setLearnErr(msg(e));
      }
    });

  const learnPending =
    learn && learn.appliesOnRestart && learnStarted !== null && learn.settings.enabled !== learnStarted
      ? learnStarted
        ? "Until then the running Hermes still records sessions."
        : "Until then the running Hermes does not record sessions."
      : null;

  const restart = io.restartHermes
    ? async () => {
        setRestarting(true);
        // What Hermes will read when it starts: the saved values now, before the restart (writes are
        // held until it ends). Committed only once the restart is confirmed.
        const readSaved = async (): Promise<HermesSavedSnapshot> => {
          const [w, l] = await Promise.all([
            webDesktop ? loadWebSettings(io.web).catch(() => null) : Promise.resolve(null),
            io.fl.trajectoryStatus().catch(() => null),
          ]);
          if (w) setWeb(w);
          if (l) setLearn(l);
          return { webSearch: w ? w.settings.searchEnabled : null, learn: l ? l.settings.enabled : null };
        };
        let r: HermesRestartResult;
        try {
          r = await withHermesStart(async (snapshot) => {
            const res = (await io.restartHermes?.()) ?? { ok: false as const, stage: "stop" as const, error: "no restart is available" };
            if (res.ok) confirmRunning(snapshot);
            return res;
          }, readSaved);
        } catch (e) {
          r = { ok: false, stage: "stop", error: msg(e) };
        }
        try {
          if (r.ok) {
            // Confirmed: Hermes stopped and started again with the snapshot taken before.
            setRestartError(null);
          } else {
            // Not confirmed: the notices stay, with what happened.
            setRestartError(
              r.stage === "stop"
                ? `Hermes could not stop (${r.error}). It is still running with its previous settings.`
                : `Hermes stopped but could not start again (${r.error}). Hermes is stopped.`,
            );
          }
        } finally {
          setRestarting(false);
        }
      }
    : null;
  const onRestart = restart ? () => void restart() : undefined;

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
        <span style={desc}>These switches change the same settings as the forms under Details. Opening this panel changes nothing.</span>
      </div>

      <Row
        row={rows["local-model"]}
        onDetails={det("local-model")}
        checked={s.chatRoute === "local_only"}
        disabled={!desktop || busy === "local-model"}
        onChange={(on) => void run("local-model", () => store.setChatRoute(on ? "local_only" : "ask_first"))}
        reason={desktop ? null : "This setting lives in the desktop app. The preview always asks before the gateway (and has none)."}
        extra={
          <span className="mono" data-testid="local-model-spend" style={note}>
            {budget
              ? `Escalation budget ${formatMicros(budget.capMicros)} a day, ${formatMicros(budget.usedMicros)} used.${budget.capMicros === 0 ? " Every paid escalation asks you first." : ""}`
              : "Escalation budget unavailable."}
          </span>
        }
      />

      <Row row={rows["web-search"]} onDetails={det("web-search")} checked={webOn} disabled={!webDesktop || web === null || busy === "web-search" || held} onChange={(on) => void setWebSearch(on)} reason={webReason} pending={webPending} onRestart={onRestart} restarting={restarting} restartError={restartError} />

      <Row row={rows.terminal} onDetails={det("terminal")} checked={!!s.hermesTerminal} disabled={busy === "terminal"} onChange={(on) => void run("terminal", () => store.setHermesTerminal(on))} />

      <Row row={rows.daemons} onDetails={det("daemons")} checked={daemonsOn} disabled={daemonsDisabled} onChange={(on) => void setDaemonsOn(on)} reason={daemonReason} />

      <Row row={rows.learn} onDetails={det("learn")} checked={learnOn} disabled={learn === null || busy === "learn" || held} onChange={(on) => void setLearnOn(on)} reason={held ? HELD_DURING_START : learnErr} pending={learnPending} onRestart={onRestart} restarting={restarting} restartError={restartError} />

      <Row row={rows.voice} onDetails={det("voice")} checked={!!s.hermesReadAloud} disabled={false} onChange={(on) => store.setHermesReadAloud(on)} />
    </div>
  );
}
