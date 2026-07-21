// =====================================================================
// citrate-core — Storage surface (1:1 from design/CitrateCore.dc.html
// "===== STORAGE =====" section). The 2.5D memory constellation
// (personal + chain-state tenants), graph search, lexical/semantic mode
// with an honest embedding-model download, tenant counts, and the local
// memory MCP endpoint.
//
// Data source — CORE-C3: the graph is served by the REAL mcp_serve daemon
// over its Unix socket. `bridge.memory.constellation()` recalls the personal +
// chain-state tenants from the per-user local store (at-rest encryption is a
// v2 item — see memory.rs; the store is NOT encrypted on disk today); nodes/labels/tenant
// counts come from that store. A deterministic layout places the real nodes.
// When the daemon is not running / the socket is unreachable, the surface shows
// an HONEST offline/empty state — never a fabricated (seed) graph (Rule 1).
// The UI is unchanged from the design; only the seed GRAPH module is replaced.
// Renders in the Instrument (dark) register — no data-register wrapper.
// =====================================================================
import { useEffect, useRef } from "react";
import { SurfaceProps } from "./shared";
import type { MemGraph } from "../shell/state";

// tenant → node colour (matches --z-green / --z-cyan; verbatim from design).
// The REAL chain tenant is "chain-state" (the daemon's name); "chain-facts" is
// kept as an alias so the design copy and any legacy label both map to cyan.
const col = (t: string) => (t === "personal" ? "#8ecc09" : "#4cc3d5");
const isChain = (t: string) => t === "chain-state" || t === "chain-facts";

const prefersReducedMotion = (): boolean => {
  try {
    return typeof window !== "undefined" && !!window.matchMedia && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  } catch {
    return false;
  }
};

/**
 * The 2.5D memory constellation. Ported faithfully from the design's
 * constellation() method: draws the link lines (every 5th a slow "comet"),
 * then each node as a soft halo + core + mono label. Pan by dragging the
 * canvas; click a node to select, click empty space to deselect. The comet
 * animation is suppressed under prefers-reduced-motion. Now driven by the REAL
 * memory graph (`s.memGraph`) instead of the seed module.
 */
function Constellation({ store, s, graph }: SurfaceProps & { graph: MemGraph }) {
  const drag = useRef<{ x: number; y: number; px: number; py: number } | null>(null);
  const reduce = prefersReducedMotion();

  const G = graph;
  const byId: Record<string, (typeof G.nodes)[number]> = {};
  G.nodes.forEach((n) => {
    byId[n.id] = n;
  });
  const tx = s.panX + 190;
  const ty = s.panY + 110;
  const gq2 = (s.graphQ || "").toLowerCase();

  return (
    <svg
      viewBox={tx + " " + ty + " 860 380"}
      style={{ position: "absolute", inset: 0, width: "100%", height: "100%", color: "var(--tx-2)", touchAction: "none" }}
      onPointerDown={(e) => {
        drag.current = { x: e.clientX, y: e.clientY, px: s.panX, py: s.panY };
        e.currentTarget.setPointerCapture(e.pointerId);
      }}
      onPointerMove={(e) => {
        if (drag.current) store.setState({ panX: drag.current.px - (e.clientX - drag.current.x), panY: drag.current.py - (e.clientY - drag.current.y) });
      }}
      onPointerUp={() => {
        drag.current = null;
      }}
      onClick={() => store.setState({ sel: null })}
    >
      {G.links.map((lk, i) => {
        const a = byId[lk[0]];
        const b = byId[lk[1]];
        if (!a || !b) return null;
        const comet = i % 5 === 0 && !reduce;
        return (
          <line
            key={"l" + i}
            x1={a.x}
            y1={a.y}
            x2={b.x}
            y2={b.y}
            stroke="currentColor"
            strokeOpacity={0.16}
            strokeWidth={1}
            style={comet ? { strokeDasharray: "10 254", animation: "ccComet " + (3.2 + (i % 4) * 0.45) + "s linear infinite" } : undefined}
          />
        );
      })}
      {G.nodes.map((n) => {
        const r = 4 * (n.z || 1) + (s.sel === n.id ? 2 : 0);
        const dim = gq2.length >= 2 && (n.label + " " + n.detail).toLowerCase().indexOf(gq2) < 0;
        return (
          <g
            key={n.id}
            transform={"translate(" + n.x + "," + n.y + ")"}
            style={{ cursor: "pointer", opacity: dim ? 0.22 : 1, transition: "opacity 220ms" }}
            onClick={(e) => {
              e.stopPropagation();
              store.setState({ sel: n.id });
            }}
          >
            <circle r={r + 7} fill={col(n.tenant)} opacity={0.1} />
            <circle r={r} fill={col(n.tenant)} stroke={s.sel === n.id ? "currentColor" : "none"} strokeWidth={1} />
            <text y={r + 13} textAnchor="middle" style={{ font: '9.5px "Geist Mono", monospace', fill: "currentColor", opacity: 0.62, pointerEvents: "none" }}>
              {n.label}
            </text>
          </g>
        );
      })}
    </svg>
  );
}

/** An honest offline/empty panel — shown when the memory daemon is not running
 * or its socket is unreachable. NEVER a sim graph (Rule 1). Includes a real Start
 * control that calls `bridge.memory.start()` via the store; on a fresh dev machine
 * the mem-mcp binary isn't bundled yet, so Start HONESTLY errors (BinaryNotFound)
 * — that error is shown, not hidden behind a fabricated "connected" state. */
function ConstellationOffline({ store, s, state }: SurfaceProps & { state: "loading" | "unavailable" | "idle" }) {
  const starting = s.memDaemon === "idle" && state !== "loading";
  const msg =
    state === "loading"
      ? "Connecting to your memory graph…"
      : s.memDaemon === "error"
        ? "Memory daemon could not start."
        : "Memory daemon offline — start it to load your graph.";
  return (
    <div style={{ position: "absolute", inset: 0, display: "flex", alignItems: "center", justifyContent: "center", flexDirection: "column", gap: 8 }}>
      <span className="mono" style={{ fontSize: 11, color: "var(--tx-3)" }}>{msg}</span>
      {state !== "loading" && (
        <button className="btn btn-ghost btn-sm" onClick={() => void store.startMemoryDaemon()}>
          {starting ? "Starting…" : "Start memory daemon"}
        </button>
      )}
      {s.memDaemon === "error" && s.memDaemonError && (
        <span className="mono" style={{ fontSize: 10, color: "var(--warn, var(--tx-3))", maxWidth: 420, textAlign: "center" }}>
          {s.memDaemonError}
        </span>
      )}
      <span className="mono" style={{ fontSize: 10, color: "var(--tx-4, var(--tx-3))" }}>no fabricated nodes are shown</span>
    </div>
  );
}

export function Storage({ store, s }: SurfaceProps) {
  // Q-A.4a: on mount, read the REAL daemon status (socket path + semantic flag +
  // supervisor state) THEN fetch the constellation. A transport failure (daemon not
  // running) is honest: memGraphState "unavailable" + memDaemon "offline" and the
  // surface shows an offline state with a Start control — never seed data (Rule 1).
  useEffect(() => {
    let cancelled = false;
    void (async () => {
      const state = await store.refreshMemoryStatus();
      if (cancelled) return;
      if (state === "running") await store.refreshConstellation();
      else store.setState({ memGraph: undefined, memGraphState: "unavailable" });
    })();
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const graph = s.memGraph;
  const ready = s.memGraphState === "ready" && !!graph;

  const selNode = ready && s.sel ? graph!.nodes.find((n) => n.id === s.sel) : null;
  const selColor = selNode ? (selNode.tenant === "personal" ? "#8ecc09" : "#4cc3d5") : "#8ecc09";

  // Q-A.4a item 1: semantic availability is a REAL read from memory_status()
  // (s.memSemantic). The bge embedding model is BUNDLED with the mem-mcp daemon —
  // there is NO UI download and NO "sha256 verified" claim on a file never fetched.
  const semanticReady = s.memSemantic;
  // Tenant totals are the REAL daemon-reported counts (never a fabricated number).
  const tenantTotal = (pred: (t: string) => boolean) =>
    (graph?.tenants ?? []).filter((t) => pred(t.tenant)).reduce((a, t) => a + t.totalInTenant, 0);
  const personalCount = tenantTotal((t) => t === "personal");
  const chainCount = tenantTotal(isChain);

  // Q-A.4a item 2: the REAL socket path from the daemon (null until known). No
  // client-invented `~/.citrate/core/memory/<persona>.sock` constant.
  const realSocket = s.memSocketPath;
  // `mcp_connect` is not a bundled shim; agents connect to the Unix socket
  // directly, so the honest config snippet points at the socket path, not a
  // command that doesn't exist.
  const onCopySnippet = () => {
    if (!realSocket) return;
    store.copy(
      JSON.stringify({ mcpServers: { "citrate-memory": { transport: "unix", socket: realSocket } } }),
      "Agent config copied",
    );
  };

  const gq = (s.graphQ || "").toLowerCase();
  const gMatches = ready && gq.length >= 2 ? graph!.nodes.filter((n) => (n.label + " " + n.detail).toLowerCase().indexOf(gq) >= 0) : [];
  const qMatches = gMatches.slice(0, 4).map((n) => ({
    id: n.id,
    label: n.label,
    dot: n.tenant === "personal" ? "#8ecc09" : "#4cc3d5",
  }));

  return (
    <div style={{ padding: "20px 26px 24px", display: "flex", flexDirection: "column", gap: 16, minHeight: "100%", boxSizing: "border-box" }}>
      <div style={{ display: "flex", alignItems: "center", gap: 18 }}>
        <span style={{ fontFamily: "var(--font-display)", fontWeight: 420, fontSize: 24 }}>Storage</span>
        <span className="mono" style={{ fontSize: 10.5, color: "var(--tx-3)" }}>
          per-user memory graph · local · yours
        </span>
        <span style={{ marginLeft: "auto", display: "flex", gap: 14 }}>
          <span style={{ display: "inline-flex", alignItems: "center", gap: 6 }}>
            <span style={{ width: 7, height: 7, borderRadius: 999, background: "var(--z-green)" }}></span>
            <span className="mono" style={{ fontSize: 10, letterSpacing: ".08em", textTransform: "uppercase", color: "var(--tx-3)" }}>
              personal
            </span>
          </span>
          <span style={{ display: "inline-flex", alignItems: "center", gap: 6 }}>
            <span style={{ width: 7, height: 7, borderRadius: 999, background: "var(--z-cyan)" }}></span>
            <span className="mono" style={{ fontSize: 10, letterSpacing: ".08em", textTransform: "uppercase", color: "var(--tx-3)" }}>
              chain-state
            </span>
          </span>
        </span>
      </div>

      <div style={{ display: "grid", gridTemplateColumns: "minmax(0,1fr) 300px", gap: 16, flex: 1, minHeight: 420 }}>
        {/* constellation canvas */}
        <div className="surface" style={{ position: "relative", overflow: "hidden", minHeight: 0, background: "var(--srf-inset)" }}>
          {ready ? (
            <Constellation store={store} s={s} graph={graph!} />
          ) : (
            <ConstellationOffline store={store} s={s} state={s.memGraphState === "ready" ? "loading" : (s.memGraphState as "loading" | "unavailable" | "idle")} />
          )}
          {selNode && (
            <div
              className="cc-fade-up"
              style={{ position: "absolute", right: 12, top: 12, width: 250, background: "var(--srf-2)", border: "1px solid var(--line-2)", borderRadius: "var(--r-2)", padding: "14px 16px", display: "flex", flexDirection: "column", gap: 8, boxShadow: "var(--shadow-lift)" }}
            >
              <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
                <span style={{ width: 7, height: 7, borderRadius: 999, background: selColor, flexShrink: 0 }}></span>
                <span className="mono" style={{ fontSize: 9.5, letterSpacing: ".12em", textTransform: "uppercase", color: "var(--tx-3)" }}>
                  {selNode.tenant} · {selNode.kind}
                </span>
                <button
                  onClick={() => store.setState({ sel: null })}
                  style={{ marginLeft: "auto", background: "none", border: "none", color: "var(--tx-3)", cursor: "pointer", fontSize: 14, lineHeight: 1, padding: 0 }}
                >
                  ×
                </button>
              </div>
              <span style={{ fontSize: 13.5, fontWeight: 500 }}>{selNode.label}</span>
              <p style={{ fontSize: 12, lineHeight: 1.55, color: "var(--tx-2)", margin: 0 }}>{selNode.detail}</p>
            </div>
          )}
          <span className="mono" style={{ position: "absolute", left: 14, bottom: 12, fontSize: 10, color: "var(--tx-3)" }}>
            drag to pan · click a node
          </span>
        </div>

        {/* right rail */}
        <div style={{ display: "flex", flexDirection: "column", gap: 12, minHeight: 0, overflow: "auto" }}>
          {/* search the graph */}
          <div className="surface" style={{ padding: 16, display: "flex", flexDirection: "column", gap: 10 }}>
            <span className="eyebrow">Search the graph</span>
            <input
              className="input"
              placeholder="recall, neighbors, contracts…"
              onInput={(e) => store.setState({ graphQ: (e.target as HTMLInputElement).value })}
              onKeyDown={(e) => {
                // Q-A.4a item 4: route Enter to the REAL daemon search
                // (bridge.memory.search) when the daemon is running — the graph is
                // re-laid from the daemon's hits, not only the client-side dim.
                if (e.key === "Enter" && s.memDaemon === "running") {
                  void store.refreshConstellation((e.target as HTMLInputElement).value);
                }
              }}
              style={{ height: 32, fontSize: 13 }}
            />
            {s.memDaemon === "running" && (
              <span className="mono" style={{ fontSize: 10, color: "var(--tx-3)" }}>
                press Enter to search the memory daemon
              </span>
            )}
            {qMatches.length > 0 && (
              <span style={{ display: "flex", flexDirection: "column", gap: 4 }}>
                {qMatches.map((qm) => (
                  <button
                    key={qm.id}
                    onClick={() => store.setState({ sel: qm.id })}
                    style={{ fontFamily: "var(--font-sans)", display: "flex", alignItems: "center", gap: 8, textAlign: "left", fontSize: 12, padding: "6px 8px", border: "1px solid var(--line-1)", borderRadius: "var(--r-1)", cursor: "pointer", background: "var(--srf-1)", color: "var(--tx-1)" }}
                  >
                    <span style={{ width: 6, height: 6, borderRadius: 999, background: qm.dot, flexShrink: 0 }}></span>
                    <span style={{ flex: 1, whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>{qm.label}</span>
                  </button>
                ))}
              </span>
            )}
          </div>

          {/* search mode — Q-A.4a item 1: semantic availability is a REAL read from
              memory_status().semantic. The bge embedding model is BUNDLED with the
              mem-mcp daemon, so there is NO UI download, NO progress bar, and NO
              "sha256 verified" claim on a file that was never fetched (Rule 1). */}
          <div className="surface" style={{ padding: 16, display: "flex", flexDirection: "column", gap: 10 }}>
            <span className="eyebrow">Search mode</span>
            <span style={{ fontSize: 13 }}>Lexical — ready now</span>
            {semanticReady ? (
              <>
                <span style={{ fontSize: 13, color: "var(--ok)" }}>Semantic — available</span>
                <span className="mono" style={{ fontSize: 11, color: "var(--tx-3)" }}>
                  bge-base · bundled with the memory daemon · first query loads the model (~2s)
                </span>
              </>
            ) : (
              <p style={{ fontSize: 11.5, lineHeight: 1.5, color: "var(--tx-3)", margin: 0 }}>
                Semantic search ships with the memory daemon's bundled embedding model — not yet
                available{s.memDaemon === "running" ? " on this build (the daemon reports lexical only)" : " (start the memory daemon to enable it)"}.
              </p>
            )}
          </div>

          {/* tenants */}
          <div className="surface" style={{ padding: 16, display: "flex", flexDirection: "column", gap: 10 }}>
            <span className="eyebrow">Tenants</span>
            <div style={{ display: "flex", justifyContent: "space-between" }}>
              <span style={{ fontSize: 12.5 }}>personal</span>
              <span className="mono tabular" style={{ fontSize: 11, color: "var(--tx-3)" }}>
                {ready ? `${personalCount} nodes` : "—"}
              </span>
            </div>
            <div style={{ display: "flex", justifyContent: "space-between" }}>
              <span style={{ fontSize: 12.5 }}>chain-state</span>
              <span className="mono tabular" style={{ fontSize: 11, color: "var(--tx-3)" }}>
                {ready ? `${chainCount} nodes` : "—"}
              </span>
            </div>
            <p style={{ fontSize: 11, lineHeight: 1.5, color: "var(--tx-3)", margin: 0, borderTop: "1px solid var(--line-1)", paddingTop: 8 }}>
              chain-state carries network params, the 40204 contract catalog, and your own wallet and validator events. Full chain-state ingest is upcoming — this is the honest v1 subset.
            </p>
          </div>

          {/* MCP endpoint — Q-A.4a item 2: the REAL socket path from the daemon
              (memory_status().socketPath). No client-invented constant; if the
              daemon isn't running there IS no socket, so we say so honestly and the
              Copy button is disabled (an agent config without a real socket would
              never connect). */}
          <div className="surface" style={{ padding: 16, display: "flex", flexDirection: "column", gap: 10 }}>
            <span className="eyebrow">MCP endpoint · connect your agents</span>
            {realSocket ? (
              <>
                <span className="mono" style={{ fontSize: 11, background: "var(--srf-inset)", border: "1px solid var(--line-1)", borderRadius: "var(--r-1)", padding: "8px 10px", wordBreak: "break-all" }}>
                  {realSocket}
                </span>
                <span>
                  <button className="btn btn-ghost btn-sm" onClick={onCopySnippet}>
                    Copy agent config
                  </button>
                </span>
                <p style={{ fontSize: 11, lineHeight: 1.5, color: "var(--tx-3)", margin: 0 }}>
                  Agents connect to this Unix socket directly. Write scope defaults to your own capability grant. Foreign processes on the socket are refused by the OS.
                </p>
              </>
            ) : (
              <>
                <span className="mono" style={{ fontSize: 11, color: "var(--tx-3)" }}>
                  start the memory daemon to get your socket path
                </span>
                <span>
                  <button className="btn btn-ghost btn-sm" onClick={() => void store.startMemoryDaemon()}>
                    Start memory daemon
                  </button>
                </span>
              </>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
