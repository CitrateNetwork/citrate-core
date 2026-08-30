// =====================================================================
// citrate-core — Agent suite (CX-S6 redesign, Pass 1)
//
// The keyless agent workbench, built 1:1 from design/CitrateCore.dc.html. A roster of attached
// runtimes (Hermes wired via bridge.agentHarness; OpenClaw / Grok as adapters, flagged pending),
// per-agent start/stop, a skills list with run-behind-approval, a session run-log, and the
// "Awaiting your review" rail. Every sensitive action an agent proposes STOPS at the Signature
// Ceremony (store.requestSig) — the agent proposes, you dispose. Nothing here fabricates data
// (Rule 1): skills/approvals come from the harness (honest-empty until it runs), and the Contracts
// tab never invents a deployed contract — a deploy is handed to the ceremony and the list stays
// empty until the network confirms.
// =====================================================================
import { useEffect, useRef, useState } from "react";
import { SurfaceProps } from "./shared";
import {
  agentSlice,
  refreshAgent,
  startRuntime,
  stopRuntime,
  attachRuntime,
  selectRuntime,
  runAgentSkill,
  clearApproval,
  noteRun,
  RUNTIME_OPTIONS,
  type RuntimeId,
} from "../shell/slices/agent";
import type { AgentApproval } from "../bridge/domains";

type Tab = "overview" | "contracts";

const KIND_COLOR: Record<AgentApproval["kind"], { fg: string; bd: string }> = {
  chain: { fg: "var(--accent-text)", bd: "var(--accent)" },
  code: { fg: "var(--info)", bd: "var(--info)" },
  shell: { fg: "var(--warn)", bd: "var(--warn)" },
};

const CONTRACT_KINDS: { id: string; name: string; desc: string }[] = [
  { id: "treasury", name: "Treasury", desc: "A group treasury — holds SALT, releases on owner/admin approval. Your agent prepares the bytecode; the deploy stops at your ceremony." },
  { id: "erc20", name: "ERC-20", desc: "A standard fungible token for your group. Name, symbol, and supply are set at deploy; nothing is minted without your signature." },
  { id: "pinvault", name: "Pin-bond vault", desc: "A vault that posts and manages storage pin bonds for the group's co-pinned files. Bonds are slashable on failed challenge." },
  { id: "custom", name: "Custom bytecode", desc: "Bring your own compiled contract. The agent simulates it on a fork; the raw deploy is decoded (or flagged raw) at the ceremony." },
];

export function Agent({ store }: SurfaceProps) {
  const st = agentSlice.use();
  const [tab, setTab] = useState<Tab>("overview");
  const [attachOpen, setAttachOpen] = useState(false);
  const [openSkill, setOpenSkill] = useState<string | null>(null);
  const skillArg = useRef<HTMLInputElement>(null);
  const [ctKind, setCtKind] = useState<string>("treasury");
  const ctName = useRef<HTMLInputElement>(null);

  useEffect(() => {
    void refreshAgent();
  }, []);

  const hermes = st.attached.find((r) => r.id === "hermes")!;
  const sel = st.attached.find((r) => r.id === st.selected) ?? hermes;
  const running = sel.wired && sel.state === "running";

  const dotColor = (state: string) =>
    state === "running" ? "var(--ok)" : state === "error" ? "var(--danger)" : state === "starting" ? "var(--warn)" : "var(--tx-3)";

  // ---- approvals: the human gate. Approve opens the ceremony; reject clears. ----
  const approve = (ap: AgentApproval) => {
    void store.requestSig({
      origin: "agent:hermes",
      requester: `agent:${sel.id} · skill runtime`,
      title: ap.summary,
      rows: [
        { k: "Kind", v: ap.kind },
        { k: "Proposed by", v: `agent:${sel.id}` },
        { k: "Effect", v: ap.kind === "chain" ? "an on-chain transaction" : ap.kind === "code" ? "a code change on your machine" : "a shell command on your machine" },
      ],
      cost: ap.kind === "chain" ? "network gas" : "—",
      sponsor: "you approve · exactly one action",
      sponsorColor: "var(--ok)",
      chainless: ap.kind !== "chain",
      apply: () => {
        clearApproval(ap.id);
        store.toast("Approved — the agent's action was witnessed once.");
      },
    });
  };
  const reject = (ap: AgentApproval) => {
    clearApproval(ap.id);
    store.toast("Rejected — nothing happened, and it won't come back.");
  };

  const runSkill = (name: string) => {
    const arg = skillArg.current?.value.trim() ?? "";
    // Malformed JSON is caught before dispatch (only when it looks like JSON).
    if (arg.startsWith("{") || arg.startsWith("[")) {
      try {
        JSON.parse(arg);
      } catch {
        store.toast("That doesn't parse as JSON — fix the arguments before running.");
        return;
      }
    }
    setOpenSkill(null);
    void runAgentSkill(name, arg);
  };

  const deploy = () => {
    const name = ctName.current?.value.trim() ?? "";
    if (!name) {
      store.toast("Name the contract first.");
      return;
    }
    const kind = CONTRACT_KINDS.find((k) => k.id === ctKind)!;
    if (ctName.current) ctName.current.value = "";
    noteRun(`deploy ${kind.name}`, name, "awaiting");
    void store.requestSig({
      origin: "agent:hermes",
      requester: `agent:${sel.id} · contract.deploy`,
      title: `Deploy “${name}” to 40204`,
      rows: [
        { k: "Template", v: kind.name },
        { k: "Name", v: name },
        { k: "Prepared by", v: `agent:${sel.id} (simulated on a fork)` },
      ],
      cost: "network gas · deployments are never sponsored",
      sponsor: "you approve · one deploy",
      sponsorColor: "var(--ok)",
      apply: () => store.toast("Deploy handed to the network — it appears under Your contracts once confirmed."),
    });
  };

  const pillTone = running
    ? { fg: "var(--ok)", bd: "var(--ok)", bg: "var(--ok-bg)", text: "running" }
    : sel.state === "error"
      ? { fg: "var(--danger)", bd: "var(--danger)", bg: "var(--danger-bg)", text: "error" }
      : { fg: "var(--tx-3)", bd: "var(--line-2)", bg: "transparent", text: "not running" };

  return (
    <div style={{ padding: "20px 26px 24px", display: "flex", flexDirection: "column", gap: 16, minHeight: "100%", boxSizing: "border-box" }}>
      {/* header */}
      <div style={{ display: "flex", alignItems: "center", gap: 12, flexWrap: "wrap" }}>
        <span style={{ fontFamily: "var(--font-display)", fontWeight: 420, fontSize: 24 }}>Agents</span>
        <span className="mono" style={{ fontSize: 9.5, letterSpacing: ".1em", textTransform: "uppercase", padding: "2px 9px", borderRadius: 999, border: "1px solid " + pillTone.bd, color: pillTone.fg, background: pillTone.bg, flexShrink: 0 }}>
          {pillTone.text}
        </span>
        <span style={{ display: "flex", gap: 2, background: "var(--srf-1)", border: "1px solid var(--line-1)", borderRadius: "var(--r-1)", padding: 2, flexShrink: 0 }}>
          {([["overview", "Overview"], ["contracts", "Contracts"]] as const).map(([id, label]) => {
            const on = tab === id;
            return (
              <button key={id} onClick={() => setTab(id)} style={{ fontFamily: "var(--font-sans)", fontSize: 12, fontWeight: on ? 500 : 400, padding: "5px 12px", border: "none", borderRadius: 5, cursor: "pointer", background: on ? "var(--srf-2)" : "transparent", color: on ? "var(--tx-1)" : "var(--tx-2)" }}>
                {label}
              </button>
            );
          })}
        </span>
        <span className="mono" style={{ marginLeft: "auto", fontSize: 10, letterSpacing: ".06em", color: "var(--tx-3)" }}>
          keyless · every sensitive action stops at the ceremony
        </span>
      </div>

      <div style={{ display: "grid", gridTemplateColumns: "minmax(0,1fr) 340px", gap: 16, alignItems: "start" }}>
        {/* ---------- main column ---------- */}
        <div style={{ display: "flex", flexDirection: "column", gap: 16, minWidth: 0 }}>
          {/* attached agents */}
          <div className="surface" style={{ display: "flex", flexDirection: "column" }}>
            <div style={{ display: "flex", alignItems: "center", padding: "12px 16px", borderBottom: "1px solid var(--line-1)" }}>
              <span style={{ fontSize: 13.5, fontWeight: 500 }}>Attached to your node</span>
              <button className="btn btn-ghost btn-sm" onClick={() => setAttachOpen((v) => !v)} style={{ marginLeft: "auto" }}>
                {attachOpen ? "Close" : "Attach agent"}
              </button>
            </div>
            {attachOpen && (
              <div style={{ display: "flex", flexDirection: "column", borderBottom: "1px solid var(--line-1)", background: "var(--srf-1)" }}>
                {RUNTIME_OPTIONS.map((ro) => {
                  const attached = st.attached.some((a) => a.id === ro.id);
                  return (
                    <div key={ro.id} style={{ display: "flex", alignItems: "center", gap: 12, padding: "11px 16px", borderBottom: "1px solid var(--line-1)" }}>
                      <span className="mono" style={{ width: 30, height: 30, borderRadius: "var(--r-1)", border: "1px solid var(--line-2)", background: "#fff", color: "var(--tx-2)", display: "inline-flex", alignItems: "center", justifyContent: "center", fontSize: 10, fontWeight: 600, flexShrink: 0 }}>
                        {ro.glyph}
                      </span>
                      <span style={{ flex: 1, minWidth: 0 }}>
                        <span style={{ display: "block", fontSize: 12.5, fontWeight: 500 }}>{ro.label}</span>
                        <span className="mono" style={{ display: "block", fontSize: 10, color: "var(--tx-3)", marginTop: 1 }}>{ro.sub}</span>
                      </span>
                      <button className="btn btn-secondary btn-sm" disabled={attached} onClick={() => { attachRuntime(ro.id); setAttachOpen(false); }}>
                        {attached ? "Attached" : "Attach"}
                      </button>
                    </div>
                  );
                })}
                <p className="mono" style={{ fontSize: 10, color: "var(--tx-3)", margin: 0, padding: "10px 16px" }}>
                  every runtime attaches keyless — whatever the agent, its chain, code, and shell actions stop at your ceremony
                </p>
              </div>
            )}
            {st.attached.map((ar) => {
              const on = ar.id === st.selected;
              return (
                <div key={ar.id} onClick={() => selectRuntime(ar.id)} style={{ display: "flex", alignItems: "center", gap: 12, padding: "11px 16px", borderBottom: "1px solid var(--line-1)", borderLeft: "2px solid " + (on ? "var(--accent)" : "transparent"), background: on ? "var(--srf-1)" : "transparent", cursor: "pointer" }}>
                  <span className="mono" style={{ width: 30, height: 30, borderRadius: "var(--r-1)", border: "1px dashed var(--line-2)", color: "var(--tx-2)", display: "inline-flex", alignItems: "center", justifyContent: "center", fontSize: 10, fontWeight: 600, flexShrink: 0 }}>
                    {ar.glyph}
                  </span>
                  <span style={{ flex: 1, minWidth: 0 }}>
                    <span style={{ display: "flex", alignItems: "center", gap: 8 }}>
                      <span style={{ width: 7, height: 7, borderRadius: 999, background: dotColor(ar.state), flexShrink: 0, animation: ar.state === "starting" ? "ccPulse 1.4s var(--ease-standard) infinite" : "none" }}></span>
                      <span style={{ fontSize: 13, fontWeight: 500 }}>{ar.label}</span>
                    </span>
                    <span className="mono" style={{ display: "block", fontSize: 10, color: "var(--tx-3)", marginTop: 2, whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>
                      {ar.state === "error" && ar.error ? ar.error : ar.sub}
                    </span>
                  </span>
                  {ar.wired && ar.state === "off" && <button className="btn btn-primary btn-sm" onClick={(e) => { e.stopPropagation(); void startRuntime(ar.id); }}>Start</button>}
                  {ar.state === "starting" && <button className="btn btn-secondary btn-sm" disabled>Starting…</button>}
                  {ar.state === "running" && <button className="btn btn-ghost btn-sm" onClick={(e) => { e.stopPropagation(); void stopRuntime(ar.id); }}>Stop</button>}
                  {!ar.wired && ar.state !== "starting" && <button className="btn btn-ghost btn-sm" onClick={(e) => { e.stopPropagation(); void startRuntime(ar.id); }}>Start</button>}
                </div>
              );
            })}
          </div>

          {tab === "overview" && (
            <>
              {/* status */}
              <div className="surface" style={{ padding: 18, display: "flex", flexDirection: "column", gap: 12 }}>
                <div style={{ display: "flex", alignItems: "center", gap: 14 }}>
                  <span style={{ width: 10, height: 10, borderRadius: 999, background: dotColor(sel.state), flexShrink: 0 }}></span>
                  <span style={{ flex: 1, minWidth: 0 }}>
                    <span style={{ display: "block", fontSize: 15, fontWeight: 500 }}>
                      {running ? `${sel.label} is running` : sel.state === "error" ? `${sel.label} couldn't start` : `${sel.label} is attached, not started`}
                    </span>
                    <span className="mono" style={{ display: "block", fontSize: 10.5, color: "var(--tx-3)", marginTop: 2 }}>
                      {running ? `${st.status.skills} skill${st.status.skills === 1 ? "" : "s"} · ${st.status.pendingApprovals} awaiting review` : sel.wired ? "start it to load its skills" : "adapter pending — Hermes is the wired runtime"}
                    </span>
                  </span>
                </div>
                {sel.state === "error" && sel.error && (
                  <div style={{ border: "1px solid var(--danger)", background: "var(--danger-bg)", borderRadius: "var(--r-1)", padding: "10px 14px", display: "flex", alignItems: "center", gap: 12 }}>
                    <span style={{ flex: 1, fontSize: 12.5, lineHeight: 1.5, color: "var(--danger)" }}>{sel.error}</span>
                    <button className="btn btn-ghost btn-sm" onClick={() => void startRuntime(sel.id)}>Retry</button>
                  </div>
                )}
                <p style={{ fontSize: 12.5, lineHeight: 1.6, color: "var(--tx-2)", margin: 0 }}>
                  This agent holds no key and signs nothing. It runs skills on your node; anything that touches the chain, your code, or your shell stops for your review.
                </p>
              </div>

              {/* skills */}
              <div className="surface" style={{ display: "flex", flexDirection: "column" }}>
                <div style={{ display: "flex", alignItems: "center", padding: "12px 16px", borderBottom: "1px solid var(--line-1)" }}>
                  <span style={{ fontSize: 13.5, fontWeight: 500 }}>Skills</span>
                  <span className="mono" style={{ marginLeft: "auto", fontSize: 9.5, letterSpacing: ".1em", textTransform: "uppercase", color: "var(--tx-3)" }}>
                    {st.skills.length} installed
                  </span>
                </div>
                {!running ? (
                  <p style={{ fontSize: 12.5, lineHeight: 1.65, color: "var(--tx-2)", margin: 0, padding: "18px 20px" }}>
                    Skills belong to a running agent. Start one above — its skills load here the moment it runs.
                  </p>
                ) : st.skills.length === 0 ? (
                  <p style={{ fontSize: 12.5, lineHeight: 1.6, color: "var(--tx-3)", margin: 0, padding: "18px 20px" }}>
                    No skills installed. Skills arrive as capsules — install one from the Commissary or drop a signed capsule into the agent's skill directory.
                  </p>
                ) : (
                  st.skills.map((sk) => {
                    const open = openSkill === sk.name;
                    return (
                      <div key={sk.name} style={{ display: "flex", flexDirection: "column", borderBottom: "1px solid var(--line-1)" }}>
                        <div style={{ display: "flex", alignItems: "center", gap: 12, padding: "12px 16px" }}>
                          <span style={{ flex: 1, minWidth: 0 }}>
                            <span className="mono" style={{ display: "block", fontSize: 12.5, fontWeight: 500 }}>{sk.name}</span>
                            <span style={{ display: "block", fontSize: 12, color: "var(--tx-2)", marginTop: 2, lineHeight: 1.5 }}>{sk.description}</span>
                          </span>
                          <button className="btn btn-ghost btn-sm" onClick={() => setOpenSkill(open ? null : sk.name)}>{open ? "Close" : "Run"}</button>
                        </div>
                        {open && (
                          <div style={{ display: "flex", gap: 10, padding: "0 16px 14px", alignItems: "center" }}>
                            <input ref={skillArg} className="input" placeholder="arguments — JSON or plain text (optional)" style={{ flex: 1 }} />
                            <button className="btn btn-primary btn-sm" onClick={() => runSkill(sk.name)}>Run skill</button>
                          </div>
                        )}
                      </div>
                    );
                  })
                )}
              </div>

              {/* recent runs */}
              <div className="surface" style={{ display: "flex", flexDirection: "column" }}>
                <div style={{ padding: "12px 16px", borderBottom: "1px solid var(--line-1)", fontSize: 13.5, fontWeight: 500 }}>Recent runs</div>
                {st.runs.length === 0 ? (
                  <p style={{ fontSize: 12.5, lineHeight: 1.6, color: "var(--tx-3)", margin: 0, padding: 16 }}>
                    Nothing yet. When you run a skill it appears here with its outcome — and a link to the approval if it needed one.
                  </p>
                ) : (
                  st.runs.map((rr) => {
                    const c = rr.status === "done" ? "var(--ok)" : rr.status === "failed" ? "var(--danger)" : rr.status === "awaiting" ? "var(--warn)" : "var(--tx-3)";
                    return (
                      <div key={rr.id} style={{ display: "flex", alignItems: "center", gap: 12, padding: "10px 16px", borderBottom: "1px solid var(--line-1)" }}>
                        <span style={{ width: 7, height: 7, borderRadius: 999, background: c, flexShrink: 0 }}></span>
                        <span style={{ flex: 1, minWidth: 0 }}>
                          <span className="mono" style={{ display: "block", fontSize: 12, fontWeight: 500 }}>{rr.name}</span>
                          <span className="mono" style={{ display: "block", fontSize: 10, color: "var(--tx-3)", marginTop: 1, whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>{rr.detail}</span>
                        </span>
                        <span className="mono" style={{ fontSize: 10, letterSpacing: ".06em", color: c, whiteSpace: "nowrap" }}>{rr.status}</span>
                      </div>
                    );
                  })
                )}
              </div>
            </>
          )}

          {tab === "contracts" && (
            <>
              <div className="surface" style={{ padding: 18, display: "flex", flexDirection: "column", gap: 12 }}>
                <span style={{ fontSize: 13.5, fontWeight: 500 }}>Deploy to 40204</span>
                <div style={{ display: "flex", gap: 6, flexWrap: "wrap" }}>
                  {CONTRACT_KINDS.map((ck) => (
                    <button key={ck.id} className={"btn btn-sm " + (ctKind === ck.id ? "btn-secondary" : "btn-ghost")} onClick={() => setCtKind(ck.id)}>
                      {ck.name}
                    </button>
                  ))}
                </div>
                <span className="mono" style={{ fontSize: 10, color: "var(--tx-3)", lineHeight: 1.6 }}>{CONTRACT_KINDS.find((k) => k.id === ctKind)!.desc}</span>
                <div style={{ display: "flex", gap: 10, alignItems: "center" }}>
                  <input ref={ctName} className="input" placeholder="Contract name — e.g. GuildTreasury" style={{ flex: 1 }} />
                  <button className="btn btn-primary btn-sm" onClick={deploy} disabled={!running} title={running ? undefined : "start an agent to prepare a deploy"}>Prepare &amp; deploy</button>
                </div>
                <span className="mono" style={{ fontSize: 10, color: "var(--tx-3)", lineHeight: 1.6 }}>
                  {running ? "your agent prepares the bytecode and simulates it — the unsigned deploy stops at your ceremony" : "start an agent above to prepare and simulate a deploy"}
                </span>
              </div>
              <div className="surface" style={{ display: "flex", flexDirection: "column" }}>
                <div style={{ display: "flex", alignItems: "center", padding: "12px 16px", borderBottom: "1px solid var(--line-1)" }}>
                  <span style={{ fontSize: 13.5, fontWeight: 500 }}>On the network</span>
                  <span className="mono" style={{ marginLeft: "auto", fontSize: 9.5, letterSpacing: ".1em", textTransform: "uppercase", color: "var(--tx-3)" }}>owned by your wallet</span>
                </div>
                <p style={{ fontSize: 12.5, lineHeight: 1.6, color: "var(--tx-3)", margin: 0, padding: 16 }}>
                  Nothing deployed yet. Pick a template above — your agent prepares the bytecode and the unsigned transaction stops at your ceremony. Confirmed deployments appear here.
                </p>
              </div>
            </>
          )}
        </div>

        {/* ---------- right rail ---------- */}
        <div style={{ display: "flex", flexDirection: "column", gap: 16 }}>
          <div className="surface" style={{ display: "flex", flexDirection: "column", borderColor: st.approvals.length ? "var(--warn)" : "var(--line-1)" }}>
            <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "12px 16px", borderBottom: "1px solid var(--line-1)" }}>
              <span style={{ fontSize: 13.5, fontWeight: 500 }}>Awaiting your review</span>
              {st.approvals.length > 0 && (
                <span className="mono tabular" style={{ fontSize: 10, minWidth: 18, height: 18, padding: "0 5px", borderRadius: 999, background: "var(--citrate-yellow)", color: "#0e0f0c", display: "inline-flex", alignItems: "center", justifyContent: "center", fontWeight: 600 }}>
                  {st.approvals.length}
                </span>
              )}
            </div>
            {st.approvals.length === 0 ? (
              <p style={{ fontSize: 12.5, lineHeight: 1.6, color: "var(--tx-3)", margin: 0, padding: 16 }}>
                Nothing pending. When the agent proposes a chain, code, or shell action, it stops here — nothing happens in your name without your say-so.
              </p>
            ) : (
              st.approvals.map((ap) => {
                const kc = KIND_COLOR[ap.kind];
                return (
                  <div key={ap.id} style={{ display: "flex", flexDirection: "column", gap: 8, padding: "12px 16px", borderBottom: "1px solid var(--line-1)" }}>
                    <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
                      <span className="mono" style={{ fontSize: 9.5, letterSpacing: ".08em", textTransform: "uppercase", padding: "2px 8px", borderRadius: 999, border: "1px solid " + kc.bd, color: kc.fg }}>{ap.kind}</span>
                      <span className="mono" style={{ fontSize: 9.5, color: "var(--tx-3)" }}>agent:{sel.id}</span>
                    </div>
                    <span style={{ fontSize: 12.5, lineHeight: 1.55 }}>{ap.summary}</span>
                    <div style={{ display: "flex", gap: 8 }}>
                      <button className="btn btn-primary btn-sm" onClick={() => approve(ap)}>Review &amp; approve</button>
                      <button className="btn btn-ghost btn-sm" onClick={() => reject(ap)}>Reject</button>
                    </div>
                  </div>
                );
              })
            )}
          </div>
          <div className="surface" style={{ padding: "16px 18px", display: "flex", flexDirection: "column", gap: 10 }}>
            <span className="eyebrow">How this stays yours</span>
            <p style={{ fontSize: 12, lineHeight: 1.65, color: "var(--tx-2)", margin: 0 }}>
              The agent proposes; you dispose. A <span className="mono" style={{ fontSize: 11 }}>chain</span> approval routes through the Signature Ceremony and produces exactly one signature. Approving once never signs twice; a rejected item never returns. Stopping the agent leaves pending items unexecuted.
            </p>
          </div>
        </div>
      </div>
    </div>
  );
}

// Silence unused-import lints for RuntimeId in builds that tree-shake types.
export type { RuntimeId };
