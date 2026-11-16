// =====================================================================
// citrate-core — Cluster (CX-S4 / CL-S2, lane s4; 0.5.4 FL design board 04)
//
// Board 04 layout: the main column has the group's cluster header, the Private mesh switch, Add a
// machine / Join with a link, the cluster map and the device list; the rail has the selected
// device, combined capacity as receipts, and "Can this machine take part?". Hardware comes only
// from this machine's probe (`fleet_probe`) and your pairings (`fleet_roster`); members' machines do
// not report hardware yet, so they show "—" with the reason (clusterBoard.ts). Eligibility is "Not
// known" until a live round publishes requirements (Group rounds not live yet).
//
// A group's private P2P cluster, DAEMON-BACKED (the citrate-cluster sidecar over UDS). The surface
// shows the group's authorized peers (only members may join — the RBAC→network boundary) with their
// LIVE connection state + the co-pinned shared-file set, all real daemon state. A peer shows online
// only when it is actually connected (Rule 1 — never a fabricated peer); a lone node meshes with
// no one yet.
// =====================================================================
import { useCallback, useEffect, useMemo, useState } from "react";
import { SurfaceProps } from "./shared";
import type { Store } from "../shell/store";
import { groupsSlice } from "../shell/slices/groups";
import {
  clusterSlice,
  loadClusterGroups,
  selectClusterGroup,
  clusterGroupLabel,
  joinCluster,
  leaveCluster,
  loadMyDevices,
  loadMemberDevices,
  revokeMyDevice,
  prepareRevokeDevice,
  cancelRevokeDevice,
  importDeviceCode,
  exportDeviceCode,
  shareMyDeviceLinks,
  syncPeerDeviceLinks,
  loadMeshStatus,
  setMeshEnabled,
} from "../shell/slices/cluster";
import { FleetWizard } from "../fleet/FleetWizard";
import { linkStatusOf, tauriFleet, type FleetApi, type FleetDevice } from "../bridge/tauri/fleet";
import { BRIDGE_MODE } from "../bridge/mode";
import { bridge } from "../bridge";
import { deviceNameError, deviceNameFrom, devicePanelModel } from "./clusterDevices";
import { peerStateLabel } from "./clusterReachability";
import { ClusterMap, THIS_MACHINE } from "./ClusterMap";
import { AddMachineSheet } from "./AddMachineSheet";
import { capacityModel, eligibilityModel, hardwareLine, peersRead, type ProbeReading, type RoundRequirements } from "./clusterBoard";
import { Card, Chip, Eyebrow, KV, Note, Unknown, type KVRow } from "../fl/primitives";
import { useFlNetworkState, type UseFlNetworkStateOptions } from "../fl/useFlNetworkState";
import { meshSwitchModel } from "./meshSwitch";

function shortAddr(a: string): string {
  if (!a) return "—";
  return a.length > 14 ? `${a.slice(0, 8)}…${a.slice(-4)}` : a;
}

/** Device addresses compare without `0x` and case. */
const sameDevice = (a: string, b: string) => a.replace(/^0x/i, "").toLowerCase() === b.replace(/^0x/i, "").toLowerCase();

/**
 * HUP-S8.1 — "Your devices": this machine's own device key and the DeviceLinks you made. Linking
 * opens the wallet ceremony (the review gate shows the exact text; nothing signs until you approve).
 * Removing a device is permanent for that device's key.
 */
function YourDevices({ store }: { store: Store }) {
  const st = clusterSlice.use();
  const [name, setName] = useState("");
  const [code, setCode] = useState("");
  const [copied, setCopied] = useState(false);
  const model = devicePanelModel(st.myDevices);
  const nameError = name ? deviceNameError(name) : null;
  const refresh = () => {
    void loadMyDevices();
    // A new link (or none, if declined) is shared with your groups; core sends nothing unchanged.
    void shareMyDeviceLinks();
    if (st.selectedId) void loadMemberDevices(st.selectedId);
  };
  return (
    <div className="surface" style={{ display: "flex", flexDirection: "column" }}>
      <div style={{ padding: "10px 16px", fontSize: 10.5, color: "var(--tx-3)", textTransform: "uppercase", letterSpacing: 0.4 }}>
        Your devices · each machine meshes under its own key
      </div>
      <p style={{ padding: "0 16px", margin: 0, fontSize: 11.5, color: "var(--tx-3)", lineHeight: 1.55 }}>
        Linking gives this machine its own key, tied to you by a link your wallet signs. The mesh can
        then tell your machines apart, and you can remove one without the others. No funds move. Link
        each of your machines: your links are shared with your groups so other members' nodes admit
        them. Removing a machine revokes its device key; a machine that still holds your wallet can
        still join your groups as you, so for a lost or stolen machine also move your funds to a new
        wallet.
      </p>
      <p data-testid="cluster-mesh-note" style={{ padding: "6px 16px 0", margin: 0, fontSize: 11.5, color: "var(--tx-3)", lineHeight: 1.55 }}>
        {st.mesh?.note ?? "Linked devices use their own key once the cross-machine mesh is on."}
      </p>
      {model.thisDeviceLinked ? (
        <div style={{ display: "flex", alignItems: "center", gap: 10, padding: "10px 16px", fontSize: 12.5, color: "var(--tx-2)" }}>
          <span style={{ flex: 1, minWidth: 0 }}>
            This device is linked as <b>{model.thisDeviceLabel}</b>.
          </span>
          <button
            className="btn btn-ghost btn-sm"
            title="Copy this device's link code to add it on another of your machines"
            onClick={async () => {
              const c = await exportDeviceCode();
              if (c && navigator.clipboard) {
                await navigator.clipboard.writeText(c);
                setCopied(true);
              }
            }}
          >
            {copied ? "Code copied" : "Copy link code"}
          </button>
        </div>
      ) : (
        <div style={{ display: "flex", alignItems: "center", gap: 10, padding: "10px 16px" }}>
          <input
            className="input"
            aria-label="Name for this device"
            placeholder="Name this device, e.g. Studio Mac"
            value={name}
            maxLength={48}
            onChange={(e) => setName(e.target.value)}
            style={{ fontSize: 12.5, flex: 1, minWidth: 0 }}
          />
          <button
            className="btn btn-primary btn-sm"
            disabled={!name.trim() || nameError !== null}
            onClick={() => void store.linkThisDevice(name.trim(), refresh)}
          >
            Link this device
          </button>
        </div>
      )}
      {nameError && (
        <div role="alert" style={{ padding: "0 16px 8px", fontSize: 11, color: "var(--bad, #c0392b)" }}>
          {nameError}
        </div>
      )}
      <div style={{ display: "flex", alignItems: "center", gap: 10, padding: "0 16px 10px" }}>
        <input
          className="input"
          aria-label="Link code from another of your devices"
          placeholder="Paste a link code from another of your devices"
          value={code}
          onChange={(e) => setCode(e.target.value)}
          style={{ fontSize: 12, flex: 1, minWidth: 0 }}
        />
        <button
          className="btn btn-ghost btn-sm"
          disabled={!code.trim()}
          onClick={async () => {
            if (await importDeviceCode(code)) setCode("");
          }}
        >
          Add device
        </button>
      </div>
      {model.rows.map((row) => (
        <div
          key={row.device}
          style={{ display: "flex", alignItems: "center", gap: 12, padding: "10px 16px", borderTop: "1px solid var(--ln, rgba(0,0,0,0.06))" }}
        >
          <span style={{ fontSize: 12.5, flex: 1, minWidth: 0 }}>
            {row.label}
            {row.thisDevice ? " (this device)" : ""}
          </span>
          <span className="mono" style={{ fontSize: 11, color: "var(--tx-3)" }}>{shortAddr(row.device)}</span>
          {st.revokePrepared && sameDevice(st.revokePrepared.device, row.device) ? (
            <>
              <span role="note" style={{ fontSize: 11, color: "var(--tx-2)", maxWidth: 320 }}>
                {st.revokePrepared.statement}
              </span>
              <button className="btn btn-ghost btn-sm" disabled={st.revoking !== null} onClick={() => cancelRevokeDevice()}>
                Keep
              </button>
              <button className="btn btn-ghost btn-sm" disabled={st.revoking !== null} onClick={() => void revokeMyDevice()}>
                Confirm: remove for good
              </button>
            </>
          ) : (
            <button className="btn btn-ghost btn-sm" disabled={st.revoking !== null} onClick={() => void prepareRevokeDevice(row.device)}>
              {st.revoking === row.device ? "Removing…" : "Remove"}
            </button>
          )}
        </div>
      ))}
    </div>
  );
}

/** Why a member's machine shows no hardware (shown after "—"): there is no source for it today. */
export const NOT_REPORTED_REASON = "not reported: members' machines do not share hardware reports yet";

const card = { padding: "14px 16px", gap: 10 } as const;

function Tile({ value, label, testId }: { value: string; label: string; testId?: string }) {
  return (
    <div data-testid={testId} style={{ display: "flex", flexDirection: "column", gap: 2 }}>
      <span className="mono tabular" style={{ fontSize: 18 }}>{value}</span>
      <span style={{ fontSize: 10.5, color: "var(--tx-3)" }}>{label}</span>
    </div>
  );
}

export interface ClusterProps {
  store: Store;
  s: SurfaceProps["s"];
  /** The fleet commands (defaults to core's). Tests inject one. */
  fleet?: FleetApi;
  /** A live round's requirements (FL lane 1). None today: Group rounds not live yet. */
  requirements?: RoundRequirements | null;
  /** Desktop app (the probe and pairing need it). Defaults to the bridge mode. */
  desktop?: boolean;
  /** The FL network-state reads (defaults to the address book and chain). Tests pass their own. */
  flNetwork?: UseFlNetworkStateOptions;
}

export function Cluster({ store, s, fleet, requirements = null, desktop = BRIDGE_MODE === "tauri", flNetwork }: ClusterProps) {
  const st = clusterSlice.use();
  const { state: fl } = useFlNetworkState(flNetwork);
  const chatGroup = groupsSlice.use().selectedId;
  const [selected, setSelected] = useState<string>(THIS_MACHINE);
  const [sheet, setSheet] = useState<"add" | "join" | null>(null);
  const [probe, setProbe] = useState<ProbeReading>({ kind: "loading" });
  const [paired, setPaired] = useState<FleetDevice[]>([]);

  useEffect(() => {
    void loadClusterGroups();
    void loadMyDevices();
    void loadMeshStatus();
    // HUP-S8.1 follow-on: share your links with your groups (nothing is sent when you have none or
    // a group already has this exact set).
    void shareMyDeviceLinks();
  }, []);

  // Follow the group open in Groups › Chat, so the four Groups tabs show the same group.
  useEffect(() => {
    if (chatGroup && chatGroup !== clusterSlice.get().selectedId) void selectClusterGroup(chatGroup);
  }, [chatGroup]);

  useEffect(() => {
    const id = st.selectedId;
    if (!id) return;
    // Pick up other members' links from the group relay first, so the roster sent with the next
    // cluster call admits their machines; then list the group's devices.
    void syncPeerDeviceLinks(id).finally(() => void loadMemberDevices(id));
  }, [st.selectedId]);

  // The wizard links this machine through the same review gate as "Link this device".
  const fleetApi = useMemo<FleetApi>(
    () =>
      fleet ?? {
        ...tauriFleet,
        linkStatus: async () => linkStatusOf(await bridge.cluster.myDevices()),
        linkThisDevice: (label: string) =>
          new Promise<void>((resolve) => {
            void store.linkThisDevice(deviceNameFrom(label), () => {
              void loadMyDevices();
              void shareMyDeviceLinks();
              resolve();
            });
          }),
      },
    [store, fleet],
  );

  const runProbe = useCallback(() => {
    if (!desktop) {
      setProbe({ kind: "failed", reason: "the hardware probe runs only in the desktop app" });
      return;
    }
    setProbe({ kind: "loading" });
    fleetApi
      .probe()
      .then((p) => setProbe({ kind: "ok", facts: p.tier.facts }))
      .catch((e) => setProbe({ kind: "failed", reason: e instanceof Error ? e.message : String(e) }));
  }, [fleetApi, desktop]);

  const loadPaired = useCallback(() => {
    if (!desktop) return;
    fleetApi
      .roster()
      .then(setPaired)
      .catch(() => setPaired([]));
  }, [fleetApi, desktop]);

  useEffect(() => {
    runProbe();
    loadPaired();
  }, [runProbe, loadPaired]);

  const memberOfDevice = new Map<string, string>();
  for (const m of st.memberDevices) for (const d of m.devices) memberOfDevice.set(d.device, d.label);

  const group = st.groups.find((g) => g.id === st.selectedId) ?? null;
  const label = group ? clusterGroupLabel(group) : null;
  const meshOn = st.mesh?.on ?? false;
  const sw = meshSwitchModel(st.mesh);
  // A loading or failed cluster read is unknown, never an empty cluster (st.peers is [] then).
  const read = peersRead(st);
  const known = read.kind === "read";
  const cap = capacityModel(probe, known ? st.peers.length : null, paired);
  const elig = eligibilityModel(probe, requirements, { phase: fl.phase, reason: fl.phaseReason });
  const peer = st.peers.find((p) => p.address === selected) ?? null;
  const sel = selected === THIS_MACHINE || !peer ? THIS_MACHINE : peer.address;

  const selectedRows: KVRow[] =
    sel === THIS_MACHINE
      ? [
          { k: "Owner", v: "You" },
          { k: "Hardware", v: probe.kind === "ok" ? hardwareLine(probe.facts) : probe.kind === "failed" ? <Unknown reason={`the probe failed: ${probe.reason}`} /> : "Reading…" },
          { k: "Link", v: "this device" },
          { k: "Reported", v: probe.kind === "ok" ? "this session, by this machine's probe" : <Unknown reason="no probe result yet" /> },
        ]
      : [
          { k: "Owner", v: peer!.member ? shortAddr(peer!.member) : shortAddr(peer!.address) },
          { k: "Hardware", v: <Unknown reason={NOT_REPORTED_REASON} /> },
          { k: "Link", v: meshOn ? peerStateLabel(peer!) : "mesh off" },
          { k: "Reported", v: "never" },
        ];

  return (
    <div className="fl-cols">
      <main className="fl-main">
        <div style={{ display: "flex", alignItems: "center", gap: 12, flexWrap: "wrap" }}>
          <h1 style={{ margin: 0, fontFamily: "var(--font-display)", fontWeight: 420, fontSize: 24 }}>{label ? `${label} cluster` : "Cluster"}</h1>
          <span style={{ flex: 1 }} />
          <label style={{ display: "inline-flex", alignItems: "center", gap: 6, fontSize: 12.5 }} title={sw.reason}>
            <input
              type="checkbox"
              role="switch"
              data-testid="cluster-mesh-switch"
              checked={sw.on}
              disabled={!sw.enabled}
              aria-describedby="cluster-mesh-reason"
              onChange={() => sw.action && void setMeshEnabled(sw.action === "turn-on")}
            />
            Private mesh
          </label>
          <button type="button" className="btn btn-sm" disabled={!desktop} onClick={() => setSheet("join")}>Join with a link</button>
          <button type="button" className="btn btn-primary btn-sm" disabled={!desktop} onClick={() => setSheet("add")}>Add a machine</button>
        </div>
        <p id="cluster-mesh-reason" style={{ margin: 0, fontSize: 11.5, color: "var(--tx-3)", lineHeight: 1.5 }}>
          {sw.reason}
          {!desktop && " Adding a machine needs the desktop app."}
        </p>

        {/* group picker */}
        <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
          <span style={{ fontSize: 12, color: "var(--tx-3)" }}>Group</span>
          {st.groups.length === 0 ? (
            <span style={{ fontSize: 12, color: "var(--tx-3)" }}>No groups yet. Create one in Groups first.</span>
          ) : (
            <select className="input" aria-label="Group" value={st.selectedId ?? ""} onChange={(e) => e.target.value && void selectClusterGroup(e.target.value)} style={{ fontSize: 12.5, minWidth: 200 }}>
              <option value="" disabled>
                Select a group…
              </option>
              {st.groups.map((g) => (
                <option key={g.id} value={g.id}>
                  {clusterGroupLabel(g)}
                </option>
              ))}
            </select>
          )}
          {st.selectedId &&
            (st.joined.includes(st.selectedId) ? (
              <button className="btn btn-ghost btn-sm" disabled={st.joining} onClick={() => st.selectedId && void leaveCluster(st.selectedId)}>Leave cluster</button>
            ) : (
              <button className="btn btn-sm" disabled={st.joining} onClick={() => st.selectedId && void joinCluster(st.selectedId)}>{st.joining ? "Joining…" : "Join cluster"}</button>
            ))}
        </div>

        {st.error && (
          <div className="surface" role="alert" style={{ padding: "12px 16px", fontSize: 12.5, color: "var(--danger)", lineHeight: 1.5 }}>
            {st.error}
          </div>
        )}

        {st.mesh && !st.mesh.on && (
          <div role="status" data-testid="cluster-mesh-off" className="surface" style={{ padding: "12px 16px", fontSize: 12.5, lineHeight: 1.55 }}>
            Mesh off on this device. Other machines can't reach it, and it can't take part in cluster rounds.
            <span style={{ display: "block", color: "var(--tx-3)", marginTop: 4 }}>{st.mesh.note}</span>
          </div>
        )}

        <div className="surface" data-testid="cluster-mesh-panel" style={{ display: "flex", flexDirection: "column" }}>
          <Eyebrow style={{ padding: "10px 16px 0" }}>
            {st.status ? `${st.status.online} of ${st.status.total} peers online · ${st.status.sharedFiles.length} shared files` : st.loading ? "Reading the cluster…" : "Cluster map"}
          </Eyebrow>
          {read.kind === "unknown" ? (
            <p data-testid="cluster-unread" role="img" aria-label={`Cluster map: unknown, ${read.reason}.`} style={{ margin: 0, padding: "8px 16px 14px", fontSize: 12.5, color: "var(--tx-2)" }}>
              Other machines: <Unknown reason={read.reason} />
            </p>
          ) : (
            <>
              {st.peers.length === 0 && (
                <p data-testid="cluster-empty" style={{ margin: 0, padding: "8px 16px 0", fontSize: 12.5, color: "var(--tx-2)" }}>
                  One machine, no peers yet. Other members' machines appear here as the group's roster admits them.
                </p>
              )}
              <ClusterMap peers={st.peers} meshOn={meshOn} selected={sel} labels={memberOfDevice} />
            </>
          )}
        </div>

        <div role="group" aria-label="Devices" style={{ display: "flex", gap: 6, flexWrap: "wrap" }}>
          <button type="button" className={"btn btn-sm" + (sel === THIS_MACHINE ? " btn-secondary" : "")} aria-pressed={sel === THIS_MACHINE} onClick={() => setSelected(THIS_MACHINE)}>
            This machine
          </button>
          {read.kind === "unknown" && (
            <span data-testid="devices-unread" style={{ fontSize: 12.5, color: "var(--tx-2)", alignSelf: "center" }}>
              Other machines: <Unknown reason={read.reason} />
            </span>
          )}
          {known && st.peers.map((p) => (
            <button key={p.address} type="button" className={"btn btn-sm" + (sel === p.address ? " btn-secondary" : "")} aria-pressed={sel === p.address} onClick={() => setSelected(p.address)}>
              {memberOfDevice.get(p.address) ?? shortAddr(p.address)} · {meshOn ? peerStateLabel(p) : "mesh off"}
            </button>
          ))}
        </div>

        <details open={!!s.pendingPairLink || undefined}>
          <summary style={{ fontSize: 12.5, cursor: "pointer", color: "var(--tx-2)" }}>Your devices and pairing</summary>
          <div style={{ display: "flex", flexDirection: "column", gap: 14, paddingTop: 12 }}>
            {/* HUP-S8.2/S8.3 — "Connect my machines": probe, opt-in discovery, link/QR pairing, Tailscale help. */}
            <FleetWizard api={fleetApi} available={desktop} initialLink={s.pendingPairLink} onInitialLinkTaken={() => store.clearPendingPairLink()} />
            <YourDevices store={store} />
            <p style={{ fontSize: 10.5, color: "var(--tx-3)", lineHeight: 1.5, margin: 0 }}>
              A Group's cluster is a private peer-to-peer mesh. Only the Group's members may join it; the roster is the admission list, enforced by the cluster daemon. Peers show online as they actually connect.
            </p>
          </div>
        </details>
      </main>

      <aside className="fl-rail" aria-label="Device and capacity">
        <Card as="section" testid="cluster-selected" style={card}>
          <Eyebrow>Selected device</Eyebrow>
          <span style={{ fontSize: 14, fontWeight: 500 }}>{sel === THIS_MACHINE ? "This machine" : memberOfDevice.get(sel) ?? shortAddr(sel)}</span>
          <KV rows={selectedRows} />
        </Card>

        <Card as="section" testid="cluster-capacity" style={card}>
          <Eyebrow>Combined capacity · receipts</Eyebrow>
          <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 10 }}>
            <Tile value={String(cap.reported)} label="machines reported" testId="cap-reported" />
            <Tile value={cap.accelerator} label="GPU memory reported" testId="cap-accel" />
            <Tile value={cap.ram} label="RAM reported" />
            <Tile value="—" label={cap.notReported === null ? "other machines unknown" : `${cap.notReported} not reported`} testId="cap-not-reported" />
          </div>
          {cap.paired.length > 0 && (
            <ul style={{ margin: 0, paddingLeft: 16, fontSize: 12, color: "var(--tx-2)", lineHeight: 1.6 }}>
              {cap.paired.map((d) => (
                <li key={d.id}>
                  {d.label} · {d.tier} (from its pairing)
                </li>
              ))}
            </ul>
          )}
          {read.kind === "unknown" && (
            <span data-testid="cap-unread" style={{ fontSize: 12 }}>
              Other machines: <Unknown reason={read.reason} />
            </span>
          )}
          <Note>Only devices that reported are counted. Unknown is not zero.</Note>
        </Card>

        <section className="fl-card" data-testid="cluster-eligibility" data-kind={elig.kind} style={{ padding: "14px 16px", display: "flex", flexDirection: "column", gap: 10, minWidth: 0 }}>
          <Eyebrow>Can this machine take part?</Eyebrow>
          <span>
            <Chip tone={elig.kind === "eligible" ? "ok" : elig.kind === "not-eligible" ? "warn" : "neutral"}>{elig.word}</Chip>
          </span>
          <ul style={{ margin: 0, paddingLeft: 16, fontSize: 12.5, lineHeight: 1.6 }}>
            {elig.lines.map((l) => (
              <li key={l}>{l}</li>
            ))}
          </ul>
          {elig.reprobe && desktop && (
            <span>
              <button type="button" className="btn btn-sm" onClick={runProbe}>Run the probe again</button>
            </span>
          )}
        </section>
      </aside>

      {sheet && (
        <AddMachineSheet
          mode={sheet}
          api={fleetApi}
          groupLabel={label}
          groupSeed={st.selectedId ? () => bridge.cluster.groupSeed(st.selectedId!) : undefined}
          addGroupSeed={st.selectedId ? (seed) => bridge.cluster.addGroupSeed(st.selectedId!, seed) : undefined}
          onClose={() => setSheet(null)}
          onPaired={loadPaired}
          mesh={st.mesh}
        />
      )}
    </div>
  );
}
