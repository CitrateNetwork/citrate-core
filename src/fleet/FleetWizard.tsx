// =====================================================================
// citrate-core — fleet wizard (HUP-S8.2 + S8.3, US-8.1 "Connect my machines")
//
// `FleetWizardView` is pure (static-render tested); `FleetWizard` wires it to the Rust commands
// through the injected `FleetApi`. Every device shown is real: this machine's probe, the local
// roster of paired machines, or a live mDNS answer. Discovery is OFF until the member turns it on,
// and is turned off again when the wizard closes. Tailscale is only ever read.
//
// HUP-S8.1 follow-on: the wallet-signed DeviceLink (made on each machine through the
// SignatureCeremony) travels with the pairing; each side stores the other's link only after core
// checks its three signatures and that it names the same member. A `citrate://pair` deep link fills
// the link in; nothing pairs until the member presses "Pair".
// =====================================================================
import { useEffect, useReducer, useRef, useState, type CSSProperties, type ReactNode } from "react";
import type { FleetApi, QrMatrix, TsState } from "../bridge/tauri/fleet";
import {
  STEPS,
  canBrowse,
  deviceRows,
  expiresIn,
  initialWizard,
  linkNote,
  needsConnectivityHelp,
  reduce,
  roleLabel,
  type Step,
  type WizardState,
} from "./wizard";

const note: CSSProperties = { fontSize: 11.5, color: "var(--tx-3)", lineHeight: 1.55, margin: 0 };
const box: CSSProperties = { padding: "14px 16px", display: "flex", flexDirection: "column", gap: 10 };

const STEP_TITLE: Record<Step, string> = {
  intro: "Start",
  machine: "This machine",
  discover: "Find machines",
  pair: "Pair",
  connect: "Connectivity",
  done: "Your machines",
};

/** A QR matrix as SVG rectangles (with the 4-module quiet zone). No image data, no innerHTML. */
export function QrCode({ qr, px = 4, testId = "fleet-qr", label = "Pairing QR code" }: { qr: QrMatrix; px?: number; testId?: string; label?: string }) {
  const q = 4;
  const n = qr.size + q * 2;
  const cells: ReactNode[] = [];
  qr.rows.forEach((row, y) => {
    for (let x = 0; x < row.length; x++) {
      if (row[x] === "1") cells.push(<rect key={`${x}-${y}`} x={x + q} y={y + q} width={1} height={1} fill="#000" />);
    }
  });
  return (
    <svg data-testid={testId} role="img" aria-label={label} viewBox={`0 0 ${n} ${n}`} width={n * px} height={n * px} shapeRendering="crispEdges">
      <rect x={0} y={0} width={n} height={n} fill="#fff" />
      {cells}
    </svg>
  );
}

const TS_STATE: Record<TsState, string> = {
  notInstalled: "Tailscale is not installed on this machine.",
  notRunning: "Tailscale is installed but not running.",
  needsLogin: "Tailscale is installed but not signed in.",
  stopped: "Tailscale is installed but turned off.",
  starting: "Tailscale is starting.",
  running: "Tailscale is connected.",
  unknown: "Tailscale's state could not be read.",
};

export interface FleetWizardHandlers {
  onStart(): void;
  onGoto(step: Step): void;
  onDiscovery(enabled: boolean): void;
  onBrowse(): void;
  onCreateLink(): void;
  onLinkInput(link: string): void;
  onInspect(): void;
  onJoin(): void;
  onRename(label: string): void;
  /** HUP-S8.1: open the wallet ceremony that links this machine. */
  onLinkDevice?(): void;
}

function DeviceList({ state }: { state: WizardState }) {
  const rows = deviceRows(state);
  if (rows.length === 0) return null;
  return (
    <div className="surface" data-testid="fleet-devices" style={{ display: "flex", flexDirection: "column" }}>
      {rows.map((r, i) => (
        <div key={r.key} style={{ display: "flex", alignItems: "center", gap: 12, padding: "10px 16px", borderTop: i ? "1px solid var(--ln, rgba(0,0,0,0.06))" : undefined }}>
          <span style={{ flex: 1, minWidth: 0, fontSize: 12.5 }}>{r.label}</span>
          <span className="mono" style={{ fontSize: 11.5 }}>{r.tier ?? "tier unknown"}</span>
          <span style={{ fontSize: 11, color: "var(--tx-2)", minWidth: 150 }}>{roleLabel(r.role)}</span>
          <span style={{ fontSize: 10.5, color: "var(--tx-3)", minWidth: 110, textAlign: "right" }}>
            {r.where}
            {r.addr ? ` · ${r.addr}` : ""}
            {r.where === "paired" ? ` · ${linkNote(r.link)}` : ""}
            {r.code ? ` · code ${r.code}` : ""}
          </span>
        </div>
      ))}
    </div>
  );
}

/** HUP-S8.1: this machine's device link, on the pair step. */
function LinkCard({ state, canLink, onLinkDevice }: { state: WizardState; canLink: boolean; onLinkDevice?: () => void }) {
  const l = state.link;
  if (!l) return null;
  return (
    <div className="surface" style={box} data-testid="fleet-link-card">
      <strong style={{ fontSize: 13 }}>Your device link</strong>
      {l.linked ? (
        <p style={note}>
          This machine is linked as <b>{l.label}</b>. Its link goes with the pairing, so your other machine lists it
          under you after checking the signatures.
        </p>
      ) : (
        <>
          <p style={note}>
            This machine is not linked yet. Link it first (your wallet signs a short text; no funds move) so the pairing
            also adds it under you on your other machine. Pairing works without it, but the machines will not mesh under
            their own keys.
          </p>
          {canLink && onLinkDevice && (
            <div>
              <button data-testid="fleet-link-device" className="btn btn-ghost btn-sm" disabled={state.busy} onClick={onLinkDevice}>
                Link this machine
              </button>
            </div>
          )}
        </>
      )}
    </div>
  );
}

export function FleetWizardView({
  state,
  nowSecs,
  available,
  canLink = false,
  ...h
}: { state: WizardState; nowSecs: number; available: boolean; canLink?: boolean } & FleetWizardHandlers) {
  const s = state;
  const [label, setLabel] = useState(s.probe?.device.label ?? "");
  const probedLabel = s.probe?.device.label;
  useEffect(() => {
    if (probedLabel) setLabel(probedLabel);
  }, [probedLabel]);

  if (!available) {
    return (
      <div className="surface" style={box}>
        <strong style={{ fontSize: 14 }}>Connect my machines</strong>
        <p style={note}>Connecting machines is available in the desktop app.</p>
      </div>
    );
  }

  const nav = (
    <div style={{ display: "flex", gap: 6, flexWrap: "wrap" }}>
      {STEPS.filter((x) => x !== "intro").map((x) => (
        <button
          key={x}
          className={`btn btn-sm ${s.step === x ? "btn-primary" : "btn-ghost"}`}
          disabled={!s.probe || s.busy}
          onClick={() => h.onGoto(x)}
        >
          {STEP_TITLE[x]}
        </button>
      ))}
    </div>
  );

  let body: ReactNode = null;
  switch (s.step) {
    case "intro":
      body = (
        <>
          <p style={note}>
            Hermes can help you connect every machine you run Citrate Core on: it checks what this machine can do,
            finds your other machines (only if you allow it), and pairs them with a one-time link or QR code.
          </p>
          <div>
            <button data-testid="fleet-start" className="btn btn-primary btn-sm" disabled={s.busy} onClick={h.onStart}>
              {s.busy ? "Checking this machine…" : "Start"}
            </button>
          </div>
        </>
      );
      break;
    case "machine": {
      const p = s.probe;
      body = p && (
        <>
          <div style={{ display: "flex", alignItems: "baseline", gap: 12 }}>
            <span style={{ fontSize: 16, fontWeight: 520 }}>{p.device.label}</span>
            <span className="mono">{p.device.tier ?? "tier unknown"}</span>
            <span style={{ fontSize: 12, color: "var(--tx-2)" }}>{roleLabel(p.device.role)}</span>
          </div>
          <ul style={{ ...note, paddingLeft: 18 }}>
            {p.tier.recommendation.rationale.map((r) => (
              <li key={r}>{r}</li>
            ))}
          </ul>
          <p style={note}>Suggested role from the hardware tier. Role names are provisional, pending owner sign-off.</p>
          <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
            <input
              className="input"
              aria-label="Name for this machine"
              value={label}
              maxLength={64}
              onChange={(e) => setLabel(e.target.value)}
              style={{ fontSize: 12.5, minWidth: 220 }}
            />
            <button className="btn btn-ghost btn-sm" disabled={s.busy || !label.trim()} onClick={() => h.onRename(label)}>
              Rename
            </button>
          </div>
          <p style={note}>Your other machines see this name while pairing. It is not your wallet or your computer's name.</p>
          <div>
            <button className="btn btn-primary btn-sm" onClick={() => h.onGoto("discover")}>Next: find my machines</button>
          </div>
        </>
      );
      break;
    }
    case "discover":
      body = (
        <>
          <label style={{ display: "flex", alignItems: "center", gap: 8, fontSize: 12.5 }}>
            <input
              data-testid="fleet-discovery-toggle"
              type="checkbox"
              checked={s.discovery.enabled}
              disabled={s.busy}
              onChange={(e) => h.onDiscovery(e.target.checked)}
            />
            Look for my other machines on this network
          </label>
          <p style={note}>
            Discovery is off until you turn it on, and turns off again when you close this wizard or restart the app.
            While it is on, machines on this network can see this machine's name, tier and role. No wallet address,
            no account, no computer name.
          </p>
          <div>
            <button data-testid="fleet-browse" className="btn btn-ghost btn-sm" disabled={!canBrowse(s) || s.busy} onClick={h.onBrowse}>
              {s.busy ? "Looking…" : "Look now"}
            </button>
          </div>
          {s.discovery.browsed && s.discovery.devices.length === 0 && (
            <p style={note}>
              No other Citrate Core machines answered. They may have discovery off, be on another network, or a firewall
              may block it.{" "}
              <button data-testid="fleet-goto-connect" className="btn btn-ghost btn-sm" onClick={() => h.onGoto("connect")}>
                Connectivity help
              </button>
            </p>
          )}
          <DeviceList state={s} />
          <div>
            <button className="btn btn-primary btn-sm" onClick={() => h.onGoto("pair")}>Next: pair a machine</button>
          </div>
        </>
      );
      break;
    case "pair":
      body = (
        <>
          <LinkCard state={s} canLink={canLink} onLinkDevice={h.onLinkDevice} />
          {s.offer?.installUrl && s.offer.installQr && (
            <div className="surface" style={box}>
              <strong style={{ fontSize: 13 }}>Citrate Core is not on the other machine yet?</strong>
              <div style={{ display: "flex", gap: 16, alignItems: "flex-start", flexWrap: "wrap" }}>
                <QrCode qr={s.offer.installQr} px={3} testId="fleet-install-qr" label="Install Citrate Core QR code" />
                <p style={note}>
                  Open{" "}
                  <a href={s.offer.installUrl} target="_blank" rel="noreferrer">
                    {s.offer.installUrl}
                  </a>{" "}
                  on that machine (or scan this code with a phone and send it the link). It picks the right installer for
                  the machine. Then open the pairing link below there.
                </p>
              </div>
            </div>
          )}
          <div className="surface" style={box}>
            <strong style={{ fontSize: 13 }}>Add another machine</strong>
            <p style={note}>
              Install Citrate Core on the other machine, then open this link there or scan the QR code. The link is
              signed by this machine, works once, and expires after 10 minutes (a default pending owner sign-off).
            </p>
            {s.offer ? (
              <div style={{ display: "flex", gap: 16, alignItems: "flex-start", flexWrap: "wrap" }}>
                <QrCode qr={s.offer.qr} />
                <div style={{ display: "flex", flexDirection: "column", gap: 6, minWidth: 0, flex: 1 }}>
                  <code className="mono" style={{ fontSize: 10.5, wordBreak: "break-all" }}>{s.offer.link}</code>
                  <span style={note}>{expiresIn(s.offer.expiresAt, nowSecs)}</span>
                  <span style={note}>Reachable at: {s.offer.hints.join(", ")}</span>
                </div>
              </div>
            ) : null}
            <div>
              <button data-testid="fleet-create-link" className="btn btn-primary btn-sm" disabled={s.busy} onClick={h.onCreateLink}>
                {s.offer ? "Create a new link" : "Create pairing link"}
              </button>
            </div>
          </div>
          <div className="surface" style={box}>
            <strong style={{ fontSize: 13 }}>I have a link from another machine</strong>
            <input
              className="input"
              aria-label="Pairing link"
              placeholder="citrate://pair?…"
              value={s.joinLink}
              onChange={(e) => h.onLinkInput(e.target.value)}
              style={{ fontSize: 12 }}
            />
            {s.inspect && (
              <p style={note}>
                Signed link from {s.inspect.issuerLabel}
                {s.inspect.issuerTier ? ` (${s.inspect.issuerTier})` : ""}, {expiresIn(s.inspect.expiresAt, nowSecs)}.
              </p>
            )}
            <div style={{ display: "flex", gap: 8 }}>
              <button className="btn btn-ghost btn-sm" disabled={s.busy || !s.joinLink.trim()} onClick={h.onInspect}>
                Check link
              </button>
              <button data-testid="fleet-join" className="btn btn-primary btn-sm" disabled={s.busy || !s.joinLink.trim()} onClick={h.onJoin}>
                {s.busy ? "Pairing…" : "Pair with this machine"}
              </button>
            </div>
          </div>
          {s.roster.length > 0 && <DeviceList state={s} />}
        </>
      );
      break;
    case "connect": {
      const ts = s.tailscale;
      body = (
        <>
          <p style={note}>
            If your machines are on different networks, Tailscale can connect them. Citrate Core only reads Tailscale's
            status; it never changes your Tailscale settings.
          </p>
          {ts ? (
            <div className="surface" style={box}>
              <strong style={{ fontSize: 13 }}>{TS_STATE[ts.report.state]}</strong>
              {ts.report.selfIps.length > 0 && <span style={note}>This machine on Tailscale: {ts.report.selfIps.join(", ")}</span>}
              {ts.report.peers.map((p) => (
                <span key={p.hostName} style={{ fontSize: 12 }}>
                  {p.hostName} · {p.os} · {p.online ? "online" : "offline"}
                </span>
              ))}
              {ts.guidance.length > 0 && (
                <ol style={{ ...note, paddingLeft: 18 }}>
                  {ts.guidance.map((g) => (
                    <li key={g.id}>
                      {g.text}
                      {g.url && (
                        <>
                          {" "}
                          <a href={g.url} target="_blank" rel="noreferrer">
                            {g.url}
                          </a>
                        </>
                      )}
                    </li>
                  ))}
                </ol>
              )}
            </div>
          ) : (
            <p style={note}>Checking Tailscale…</p>
          )}
          <div style={{ display: "flex", gap: 8 }}>
            <button className="btn btn-primary btn-sm" onClick={() => h.onGoto("pair")}>Back to pairing</button>
            <button className="btn btn-ghost btn-sm" onClick={() => h.onGoto("connect")}>Check again</button>
          </div>
        </>
      );
      break;
    }
    case "done":
      body = (
        <>
          <DeviceList state={s} />
          <p style={note}>
            Paired machines are recorded on each machine. When both machines are linked, each one stores the other's
            wallet-signed device link after checking it, so the cluster lists both under you with their own keys. A
            machine that was not linked can be linked now and paired again.
          </p>
          <p style={note}>To share with other people, create a group and send invites in Groups.</p>
          <div>
            <button className="btn btn-ghost btn-sm" onClick={() => h.onGoto("pair")}>Pair another machine</button>
          </div>
        </>
      );
      break;
  }

  return (
    <div className="surface" data-testid="fleet-wizard" style={{ ...box, gap: 12 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 12 }}>
        <strong style={{ fontSize: 14 }}>Connect my machines</strong>
        <span style={{ marginLeft: "auto", fontSize: 11, color: "var(--tx-3)" }}>{STEP_TITLE[s.step]}</span>
      </div>
      {s.step !== "intro" && nav}
      {s.error && (
        <div role="alert" style={{ fontSize: 12.5, color: "var(--bad, #c0392b)", lineHeight: 1.5 }}>
          {s.error}
        </div>
      )}
      {body}
      {s.step !== "connect" && s.step !== "intro" && needsConnectivityHelp(s) && s.step !== "discover" && (
        <p style={note}>
          Trouble reaching a machine?{" "}
          <button className="btn btn-ghost btn-sm" onClick={() => h.onGoto("connect")}>
            Connectivity help
          </button>
        </p>
      )}
    </div>
  );
}

const msg = (e: unknown) => (e instanceof Error ? e.message : String(e));

/** How often the issuing machine re-reads its roster while a pairing link is open. */
const ROSTER_POLL_MS = 3_000;

/**
 * The wired wizard. `api` is the Tauri bridge in the app; `available` is false outside it.
 * `initialLink` is a pairing link handed in by a `citrate://pair` deep link: the wizard probes this
 * machine, fills the link in on the pair step, and reports it taken. It never joins by itself.
 */
export function FleetWizard({
  api,
  available,
  initialLink,
  onInitialLinkTaken,
}: {
  api: FleetApi;
  available: boolean;
  initialLink?: string | null;
  onInitialLinkTaken?: () => void;
}) {
  const [state, dispatch] = useReducer(reduce, undefined, initialWizard);
  const [now, setNow] = useState(() => Math.floor(Date.now() / 1000));
  const discoveryOn = useRef(false);
  discoveryOn.current = state.discovery.enabled;

  const readLinkStatus = async () => {
    if (!api.linkStatus) return;
    try {
      dispatch({ type: "linkStatus", status: await api.linkStatus() });
    } catch {
      /* no device key / keyring locked: the card stays hidden rather than guessing */
    }
  };

  // A deep-linked pairing link: probe, then fill it in on the pair step (the member presses Pair).
  const takenLink = useRef<string | null>(null);
  useEffect(() => {
    if (!available || !initialLink || takenLink.current === initialLink) return;
    takenLink.current = initialLink;
    void (async () => {
      dispatch({ type: "start" });
      try {
        const [probe, roster] = await Promise.all([api.probe(), api.roster()]);
        dispatch({ type: "roster", devices: roster });
        dispatch({ type: "probed", probe });
        await readLinkStatus();
      } catch (e) {
        dispatch({ type: "failed", error: msg(e) });
      }
      dispatch({ type: "prefill", link: initialLink });
      onInitialLinkTaken?.();
    })();
  }, [initialLink, available]);

  // While a pairing link is open, refresh the list so the machine that joins shows up here too.
  const offerOpenUntil = state.offer?.expiresAt ?? 0;
  useEffect(() => {
    if (!offerOpenUntil) return;
    const t = setInterval(() => {
      if (Math.floor(Date.now() / 1000) >= offerOpenUntil) return;
      void api
        .roster()
        .then((devices) => dispatch({ type: "roster", devices }))
        .catch(() => undefined);
    }, ROSTER_POLL_MS);
    return () => clearInterval(t);
  }, [api, offerOpenUntil]);

  useEffect(() => {
    const t = setInterval(() => setNow(Math.floor(Date.now() / 1000)), 15_000);
    return () => {
      clearInterval(t);
      // Consent is scoped to the wizard: leaving it turns discovery off.
      if (discoveryOn.current) void api.setDiscovery(false).catch(() => undefined);
    };
  }, [api]);

  const act = async (f: () => Promise<void>) => {
    dispatch({ type: "start" });
    try {
      await f();
    } catch (e) {
      dispatch({ type: "failed", error: msg(e) });
    }
  };

  const loadTailscale = (unreachable: boolean) =>
    act(async () => {
      const view = await api.tailscale(state.discovery.devices.length, unreachable);
      dispatch({ type: "tailscale", view });
    });

  const handlers: FleetWizardHandlers = {
    onStart: () =>
      void act(async () => {
        const [probe, roster] = await Promise.all([api.probe(), api.roster()]);
        dispatch({ type: "roster", devices: roster });
        dispatch({ type: "probed", probe });
        await readLinkStatus();
      }),
    onLinkDevice: api.linkThisDevice
      ? () =>
          void act(async () => {
            const label = (state.probe?.device.label ?? "").trim().slice(0, 48);
            await api.linkThisDevice?.(label);
            await readLinkStatus();
          })
      : undefined,
    onGoto: (step) => {
      dispatch({ type: "goto", step });
      if (step === "connect") void loadTailscale(state.unreachable || needsConnectivityHelp(state));
    },
    onDiscovery: (enabled) =>
      void act(async () => {
        const r = await api.setDiscovery(enabled);
        dispatch({ type: "discoverySet", enabled: r.enabled });
      }),
    onBrowse: () =>
      void act(async () => {
        dispatch({ type: "browsed", devices: await api.browse() });
      }),
    onCreateLink: () =>
      void act(async () => {
        dispatch({ type: "offer", offer: await api.createLink() });
      }),
    onLinkInput: (link) => dispatch({ type: "linkInput", link }),
    onInspect: () =>
      void act(async () => {
        dispatch({ type: "inspected", claim: await api.inspectLink(state.joinLink.trim()) });
      }),
    onJoin: () =>
      void act(async () => {
        const result = await api.joinLink(state.joinLink.trim());
        dispatch({ type: "joined", result });
        if (result.errorKind === "unreachable") {
          const view = await api.tailscale(state.discovery.devices.length, true);
          dispatch({ type: "tailscale", view });
        }
      }),
    onRename: (label) =>
      void act(async () => {
        await api.setLabel(label);
        dispatch({ type: "probed", probe: await api.probe() });
      }),
  };

  return <FleetWizardView state={state} nowSecs={now} available={available} canLink={!!api.linkThisDevice} {...handlers} />;
}
