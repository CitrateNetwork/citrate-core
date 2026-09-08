import markBlack from "../assets/brand/citrate_mark_black.svg";
import { LoaderMark } from "../components/LoaderMark";
import { Store } from "./store";
import { AppState, ORIGIN_COLORS, PERSONAS, short } from "./state";
import { COACH_STEPS } from "../data/seed";
import { BRIDGE_MODE } from "../bridge/mode";

// ============================ SIGNATURE CEREMONY ============================
export function SignatureCeremony({ store, s }: { store: Store; s: AppState }) {
  const head = s.queue[0];
  if (!head) return null;
  const originColor = ORIGIN_COLORS[head.origin] || "var(--tx-2)";
  const chainless = head.chainless;
  const stepLabels = chainless
    ? ["recording your approval", "writing to the local store", "checkpointing"]
    : ["signing — keystore in OS keyring", "broadcasting UserOp", "awaiting inclusion"];

  return (
    <div data-register="charter" style={{ position: "fixed", inset: 0, background: "rgba(14,15,12,.44)", display: "flex", alignItems: "center", justifyContent: "center", zIndex: 60, padding: 24 }}>
      <div className="cc-fade-up" style={{ width: "100%", maxWidth: 480, background: "#ffffff", border: "1px solid var(--line-2)", borderRadius: "var(--r-3)", boxShadow: "var(--shadow-lift)", overflow: "hidden", display: "flex", flexDirection: "column" }}>
        <div style={{ display: "flex", alignItems: "center", gap: 10, padding: "14px 20px", borderBottom: "2px solid var(--line-strong)" }}>
          <img src={markBlack} alt="" style={{ width: 18, height: 18 }} />
          <span className="mono" style={{ fontSize: 10, letterSpacing: ".14em", textTransform: "uppercase", color: "var(--tx-2)" }}>
            Signature ceremony
          </span>
          <span style={{ flex: 1 }}></span>
          <span className="mono" style={{ fontSize: 9.5, letterSpacing: ".1em", textTransform: "uppercase", padding: "2px 9px", borderRadius: 999, border: "1px solid " + originColor, color: originColor }}>
            {head.origin}
          </span>
        </div>

        {s.cerPhase === "review" && (
          <div style={{ padding: "18px 20px", display: "flex", flexDirection: "column", gap: 14 }}>
            <div>
              <div style={{ fontFamily: "var(--font-display)", fontWeight: 440, fontSize: 20, lineHeight: 1.2 }}>{head.title}</div>
              <div className="mono" style={{ fontSize: 11, color: "var(--tx-3)", marginTop: 4 }}>
                requested by {head.requester}
              </div>
            </div>
            <div style={{ border: "1px solid var(--line-1)", borderRadius: "var(--r-1)", overflow: "hidden" }}>
              {head.rows.map((cr, i) => (
                <div key={i} style={{ display: "flex", gap: 14, padding: "9px 14px", borderBottom: "1px solid var(--line-1)", background: "var(--srf-1)" }}>
                  <span className="mono" style={{ fontSize: 9.5, letterSpacing: ".1em", textTransform: "uppercase", color: "var(--tx-3)", width: 112, flexShrink: 0, paddingTop: 2 }}>
                    {cr.k}
                  </span>
                  <span className="mono" style={{ fontSize: 12, color: "var(--tx-1)", wordBreak: "break-all" }}>
                    {cr.v}
                  </span>
                </div>
              ))}
              <div style={{ display: "flex", gap: 14, padding: "9px 14px", background: "var(--srf-1)" }}>
                <span className="mono" style={{ fontSize: 9.5, letterSpacing: ".1em", textTransform: "uppercase", color: "var(--tx-3)", width: 112, flexShrink: 0, paddingTop: 2 }}>
                  Cost
                </span>
                <span style={{ display: "flex", flexDirection: "column", gap: 2 }}>
                  <span className="mono" style={{ fontSize: 12 }}>{head.cost}</span>
                  <span className="mono" style={{ fontSize: 10.5, color: head.sponsorColor || "var(--tx-3)" }}>
                    {head.sponsor}
                  </span>
                </span>
              </div>
            </div>
            {head.warning && (
              <div style={{ border: "1px solid var(--warn)", background: "var(--warn-bg)", borderRadius: "var(--r-1)", padding: "10px 14px" }}>
                <span style={{ fontSize: 12, lineHeight: 1.5, color: "var(--warn)" }}>{head.warning}</span>
              </div>
            )}
            <div style={{ display: "flex", gap: 10, justifyContent: "flex-end", paddingTop: 2 }}>
              <button
                className="btn btn-ghost"
                ref={(el) => {
                  if (el && head && s.cerPhase === "review") {
                    try {
                      el.focus();
                    } catch {
                      /* ignore */
                    }
                  }
                }}
                onClick={() => {
                  store.finishCer("declined");
                  store.toast("Declined — nothing was signed");
                }}
              >
                Decline
              </button>
              <button className="btn btn-primary" onClick={() => store.approveCer()}>
                Approve &amp; sign
              </button>
            </div>
            {s.queue.length > 1 && (
              <div className="mono" style={{ fontSize: 10, color: "var(--tx-3)", textAlign: "right" }}>
                {s.queue.length - 1} more request{s.queue.length > 2 ? "s" : ""} waiting
              </div>
            )}
          </div>
        )}

        {s.cerPhase === "busy" && (
          <div style={{ padding: "30px 20px", display: "flex", flexDirection: "column", alignItems: "center", gap: 16 }}>
            <div style={{ width: 84, height: 84 }}>
              <LoaderMark size={84} />
            </div>
            <div style={{ display: "flex", flexDirection: "column", gap: 8, width: "100%", maxWidth: 280 }}>
              {stepLabels.map((label, i) => {
                const on = s.cerStep > i;
                const active = s.cerStep === i;
                return (
                  <div key={i} style={{ display: "flex", alignItems: "center", gap: 10 }}>
                    <span
                      style={{
                        width: 16,
                        height: 16,
                        borderRadius: 999,
                        display: "inline-flex",
                        alignItems: "center",
                        justifyContent: "center",
                        background: on ? "var(--ok-bg)" : "transparent",
                        border: "1px solid " + (on ? "var(--ok)" : active ? "var(--tx-2)" : "var(--line-2)"),
                        color: on ? "var(--ok)" : "transparent",
                        flexShrink: 0,
                      }}
                    >
                      <svg viewBox="0 0 24 24" width="10" height="10" fill="none" stroke="currentColor" strokeWidth="3" strokeLinecap="round">
                        <path d="M5 12 L10 17 L19 8"></path>
                      </svg>
                    </span>
                    <span className="mono" style={{ fontSize: 11.5, color: on || active ? "var(--tx-1)" : "var(--tx-3)" }}>
                      {label}
                    </span>
                  </div>
                );
              })}
            </div>
          </div>
        )}

        {s.cerPhase === "done" && (
          <div style={{ padding: "26px 20px", display: "flex", flexDirection: "column", alignItems: "center", gap: 12 }}>
            <span className="cc-stamp" style={{ width: 44, height: 44, borderRadius: 999, background: "var(--ok-bg)", border: "1.5px solid var(--ok)", color: "var(--ok)", display: "inline-flex", alignItems: "center", justifyContent: "center" }}>
              <svg viewBox="0 0 24 24" width="22" height="22" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round">
                <path d="M5 12 L10 17 L19 8"></path>
              </svg>
            </span>
            <span style={{ fontSize: 15, fontWeight: 500 }}>Witnessed</span>
            <span className="mono" style={{ fontSize: 11, color: "var(--accent-text)" }}>
              {short(s.cerHash)}
            </span>
          </div>
        )}
      </div>
    </div>
  );
}

// ================= WALLET REVIEW MODAL (Q-E.1, @rule8, P0) =================
// The human-in-the-loop gate for wallet money actions. A money action builds the
// pending ceremony and STOPS; this modal renders the DECODED CeremonyView (what
// will be signed — action / destination / cost) with explicit Approve + Reject.
// Approve is NEVER default-focused (T2 ceremony-spoofing invariant); undecodable
// calldata (requiresRawAck) hides the decoded fields behind an explicit raw-mode
// ack that must be ticked before Approve enables. On Approve → store broadcasts;
// on Reject → nothing is signed. In web-dev the modal still shows (truthful flow)
// but approving honestly reports "settles only in the desktop app".
export function WalletReviewModal({ store, s }: { store: Store; s: AppState }) {
  const r = s.walletReview;
  if (!r) return null;
  const v = r.view;
  const raw = v.requiresRawAck;
  const approveEnabled = !raw || r.rawAck;

  const rows: [string, string][] = [
    ["Action", v.decoded.action || "—"],
    ["To", v.decoded.destination || "—"],
    ["Cost", v.decoded.cost || (BRIDGE_MODE === "tauri" ? "network gas" : "gas settles in the desktop app")],
    ["Chain", String(v.chainId)],
    ["Origin", v.origin],
  ];

  return (
    <div data-register="charter" style={{ position: "fixed", inset: 0, background: "rgba(14,15,12,.44)", display: "flex", alignItems: "center", justifyContent: "center", zIndex: 62, padding: 24 }}>
      <div className="cc-fade-up" role="dialog" aria-modal="true" aria-label="Wallet action review" style={{ width: "100%", maxWidth: 480, background: "#ffffff", border: "1px solid var(--line-2)", borderRadius: "var(--r-3)", boxShadow: "var(--shadow-lift)", overflow: "hidden", display: "flex", flexDirection: "column" }}>
        <div style={{ display: "flex", alignItems: "center", gap: 10, padding: "14px 20px", borderBottom: "2px solid var(--line-strong)" }}>
          <img src={markBlack} alt="" style={{ width: 18, height: 18 }} />
          <span className="mono" style={{ fontSize: 10, letterSpacing: ".14em", textTransform: "uppercase", color: "var(--tx-2)" }}>
            Review &amp; approve
          </span>
          <span style={{ flex: 1 }}></span>
          <span className="mono" style={{ fontSize: 9.5, letterSpacing: ".1em", textTransform: "uppercase", padding: "2px 9px", borderRadius: 999, border: "1px solid var(--tx-2)", color: "var(--tx-2)" }}>
            {r.label}
          </span>
        </div>

        <div style={{ padding: "18px 20px", display: "flex", flexDirection: "column", gap: 14 }}>
          <div>
            <div style={{ fontFamily: "var(--font-display)", fontWeight: 440, fontSize: 20, lineHeight: 1.2 }}>You are about to sign</div>
            <div className="mono" style={{ fontSize: 11, color: "var(--tx-3)", marginTop: 4 }}>
              nothing has been signed yet — approve to continue
            </div>
          </div>

          {raw ? (
            // Undecodable calldata — Rule 1: DO NOT dress it as legible. Show the raw
            // warning + require an explicit ack before Approve enables.
            <div style={{ border: "1px solid var(--warn)", background: "var(--warn-bg)", borderRadius: "var(--r-1)", padding: "12px 14px", display: "flex", flexDirection: "column", gap: 8 }}>
              <span style={{ fontSize: 12.5, lineHeight: 1.5, color: "var(--warn)" }}>
                This action&apos;s calldata could not be decoded. You would be signing raw bytes — the app cannot show you what they do. Only proceed if you trust the source.
              </span>
              <label style={{ display: "flex", alignItems: "center", gap: 8, fontSize: 12, cursor: "pointer" }}>
                <input type="checkbox" checked={r.rawAck} onChange={(e) => store.setWalletReviewRawAck(e.target.checked)} />
                <span>I understand this is raw, undecodable calldata.</span>
              </label>
              <div className="mono" style={{ fontSize: 11, color: "var(--tx-3)", wordBreak: "break-all" }}>
                origin {v.origin} · chain {v.chainId}
              </div>
            </div>
          ) : (
            <div style={{ border: "1px solid var(--line-1)", borderRadius: "var(--r-1)", overflow: "hidden" }}>
              {rows.map(([k, val], i) => (
                <div key={i} style={{ display: "flex", gap: 14, padding: "9px 14px", borderBottom: i < rows.length - 1 ? "1px solid var(--line-1)" : "none", background: "var(--srf-1)" }}>
                  <span className="mono" style={{ fontSize: 9.5, letterSpacing: ".1em", textTransform: "uppercase", color: "var(--tx-3)", width: 92, flexShrink: 0, paddingTop: 2 }}>
                    {k}
                  </span>
                  <span className="mono" style={{ fontSize: 12, color: "var(--tx-1)", wordBreak: "break-all" }}>
                    {val}
                  </span>
                </div>
              ))}
            </div>
          )}

          {BRIDGE_MODE !== "tauri" && (
            <div style={{ border: "1px solid var(--line-1)", background: "var(--srf-1)", borderRadius: "var(--r-1)", padding: "10px 14px" }}>
              <span style={{ fontSize: 11.5, lineHeight: 1.5, color: "var(--tx-3)" }}>
                Web preview: this settles only in the desktop app (no key or chain here). Approving will not move funds.
              </span>
            </div>
          )}

          <div style={{ display: "flex", gap: 10, justifyContent: "flex-end", paddingTop: 2 }}>
            {/* Reject is the DEFAULT-focused button (T2 — Approve must never be the
                default action for a money signature). */}
            <button
              className="btn btn-ghost"
              ref={(el) => {
                if (el && s.walletReview) {
                  try {
                    el.focus();
                  } catch {
                    /* ignore */
                  }
                }
              }}
              onClick={() => void store.rejectWalletReview()}
            >
              Reject
            </button>
            <button className="btn btn-primary" disabled={!approveEnabled} onClick={() => void store.approveWalletReview()}>
              Approve
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}

// ============================ COACH ============================
export function Coach({ store, s }: { store: Store; s: AppState }) {
  const steps = COACH_STEPS;
  const head = s.queue[0];
  const show = s.coach >= 0 && s.coach < steps.length && !!steps.length && s.stage === "done" && !head;
  if (!show) return null;
  const cstep = steps[s.coach] || { title: "", body: "" };
  const nextLabel = s.coach >= steps.length - 1 ? "Done" : "Next";
  return (
    <div data-register="charter" style={{ position: "fixed", inset: 0, background: "rgba(14,15,12,.38)", display: "flex", alignItems: "flex-end", justifyContent: "center", zIndex: 50, padding: 40 }}>
      <div className="cc-fade-up" style={{ width: "100%", maxWidth: 440, background: "#ffffff", border: "1px solid var(--line-2)", borderRadius: "var(--r-3)", boxShadow: "var(--shadow-lift)", padding: "20px 22px", display: "flex", flexDirection: "column", gap: 10 }}>
        <span className="mono" style={{ fontSize: 9.5, letterSpacing: ".13em", textTransform: "uppercase", color: "var(--tx-3)" }}>
          First run · {s.coach + 1} of {steps.length}
        </span>
        <span style={{ fontFamily: "var(--font-display)", fontWeight: 440, fontSize: 19, lineHeight: 1.25 }}>{cstep.title}</span>
        <p style={{ fontSize: 13, lineHeight: 1.6, color: "var(--tx-2)", margin: 0 }}>{cstep.body}</p>
        <div style={{ display: "flex", gap: 10, justifyContent: "flex-end", marginTop: 4, alignItems: "center" }}>
          {/* CX-S7.2 — a step's optional CTA navigates the user to the surface it names (e.g. the
              groups step opens Groups, so a non-technical user actually reaches a Group). Advancing
              the coach as it navigates keeps the walk moving. */}
          {cstep.cta && (
            <button
              className="btn btn-sm"
              style={{ marginRight: "auto" }}
              onClick={() => {
                store.go(cstep.cta!.route);
                const nx = s.coach + 1;
                if (nx >= steps.length) store.setState({ coach: -1, coachDone: true });
                else store.setState({ coach: nx });
                store.save();
              }}
            >
              {cstep.cta.label}
            </button>
          )}
          <button className="btn btn-ghost btn-sm" onClick={() => { store.setState({ coach: -1, coachDone: true }); store.save(); }}>
            Skip
          </button>
          <button
            className="btn btn-secondary btn-sm"
            onClick={() => {
              const nx = s.coach + 1;
              if (nx >= steps.length) store.setState({ coach: -1, coachDone: true });
              else store.setState({ coach: nx });
              store.save();
            }}
          >
            {nextLabel}
          </button>
        </div>
      </div>
    </div>
  );
}

// ============================ TOAST ============================
export function Toast({ s }: { s: AppState }) {
  if (!s.toast) return null;
  return (
    <div data-register="charter" style={{ position: "fixed", left: "50%", bottom: 24, transform: "translateX(-50%)", zIndex: 80 }}>
      <div className="cc-fade-up" style={{ display: "flex", alignItems: "center", gap: 10, background: "#0e0f0c", color: "#f4f1ea", borderRadius: "var(--r-2)", padding: "10px 18px", boxShadow: "var(--shadow-lift)" }}>
        <span style={{ width: 6, height: 6, borderRadius: 999, background: "var(--citrate-green)", flexShrink: 0 }}></span>
        <span style={{ fontSize: 12.5 }}>{s.toast}</span>
      </div>
    </div>
  );
}

// ============================ DEMO PANEL ============================
export function DemoPanel({ store, s }: { store: Store; s: AppState }) {
  const btnCls = (on: boolean) => "btn btn-sm " + (on ? "btn-secondary" : "btn-ghost");
  // The prototype/demo controls (the floating "Prototype" tag + its panel) are a
  // web-dev affordance only — hidden in the packaged app (BRIDGE_MODE === "tauri").
  const showProto = BRIDGE_MODE === "sim";
  return (
    <>
      {showProto && s.demoOpen && (
        <div data-register="charter" className="sim-proto-affordance" style={{ position: "fixed", right: 18, bottom: 64, width: 320, background: "#ffffff", border: "1px solid var(--line-2)", borderRadius: "var(--r-3)", boxShadow: "var(--shadow-lift)", zIndex: 70, display: "flex", flexDirection: "column", overflow: "hidden" }}>
          <div style={{ display: "flex", alignItems: "center", padding: "12px 16px", borderBottom: "1px solid var(--line-1)" }}>
            <span className="mono" style={{ fontSize: 10, letterSpacing: ".14em", textTransform: "uppercase", color: "var(--tx-2)" }}>
              Prototype controls
            </span>
            <button onClick={() => store.setState({ demoOpen: false })} style={{ marginLeft: "auto", background: "none", border: "none", color: "var(--tx-3)", cursor: "pointer", fontSize: 16, lineHeight: 1, padding: 0 }}>
              ×
            </button>
          </div>
          <div style={{ padding: "14px 16px", display: "flex", flexDirection: "column", gap: 14, maxHeight: "60vh", overflow: "auto" }}>
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
              <span className="lbl">Persona</span>
              {Object.keys(PERSONAS).map((pid) => {
                const p = PERSONAS[pid];
                const active = s.persona === pid;
                return (
                  <button
                    key={pid}
                    onClick={() => store.selectPersona(pid)}
                    style={{ fontFamily: "var(--font-sans)", textAlign: "left", fontSize: 12, padding: "8px 10px", border: "1px solid " + (active ? "var(--tx-1)" : "var(--line-1)"), borderRadius: "var(--r-1)", cursor: "pointer", background: active ? "var(--srf-1)" : "transparent", color: "var(--tx-1)" }}
                  >
                    <span style={{ fontWeight: active ? 500 : 400 }}>{p.label}</span>
                    <span style={{ display: "block", fontSize: 10.5, color: "var(--tx-3)", marginTop: 1 }}>{p.blurb}</span>
                  </button>
                );
              })}
            </div>
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
              <span className="lbl">Entitlement state</span>
              <span style={{ display: "flex", gap: 6, flexWrap: "wrap" }}>
                {(["active", "expiring", "grace", "lapsed"] as const).map((e) => (
                  <button key={e} className={btnCls(s.entitlement === e)} onClick={() => { store.setState({ entitlement: e }); store.save(); }}>
                    {e}
                  </button>
                ))}
              </span>
            </div>
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
              <span className="lbl">Chat backend</span>
              <span style={{ display: "flex", gap: 6 }}>
                <button className={btnCls(s.chatBackend === "gateway")} onClick={() => { store.setState({ chatBackend: "gateway" }); store.save(); }}>
                  gateway
                </button>
                <button className={btnCls(s.chatBackend === "local")} onClick={() => { store.setState({ chatBackend: "local" }); store.save(); }}>
                  local fallback
                </button>
              </span>
            </div>
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
              <span className="lbl">Trigger a signature request</span>
              <span style={{ display: "flex", gap: 6, flexWrap: "wrap" }}>
                <button
                  className="btn btn-ghost btn-sm"
                  onClick={() => {
                    store.setState({ demoOpen: false });
                    store.requestSig({
                      origin: "node-agent",
                      requester: "node-agent · PIN planner 127.0.0.1:19600",
                      title: "Bond 120 SALT to pin a CID",
                      rows: [
                        { k: "Action", v: "plan_pin(bafybeigd4x…kq4e) → bond" },
                        { k: "Bond", v: "120 SALT · slashable on failed challenge" },
                        { k: "Projected", v: "~0.9 SALT / month at current demand" },
                      ],
                      cost: "est. gas 0.0019 SALT",
                      sponsor: "gas sponsored — standard daily budget",
                      sponsorColor: "var(--ok)",
                      apply: (h) => {
                        store.addActivity("Pin bond", "−120.00 SALT", h);
                        store.setState((st) => ({ liquid: Math.max(0, st.liquid - 120), pins: [{ cid: "bafybeigd4x…kq4e", bond: 120, cadH: 6, nextIn: 6 * 3600, last: "attested" }].concat(st.pins) }));
                        store.toast("Bond posted — challenges begin next window");
                      },
                    });
                  }}
                >
                  node-agent · pin bond
                </button>
                <button
                  className="btn btn-ghost btn-sm"
                  onClick={() => {
                    store.setState({ demoOpen: false });
                    store.openMicroApp({ name: "Lattice Observatory", capabilities: ["identity token hand-off", "EIP-1193 provider (read + propose)"] });
                  }}
                >
                  micro-app · provider
                </button>
              </span>
            </div>
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
              <span className="lbl">KYC outcome at S2</span>
              <span style={{ display: "flex", gap: 6, flexWrap: "wrap" }}>
                {([["verified", "verified"], ["failed", "failed"], ["review", "manual review"]] as const).map(([id, label]) => (
                  <button key={id} className={btnCls(s.kycOutcome === id)} onClick={() => { store.setState({ kycOutcome: id }); store.toast('S2 will resolve to “' + label + '”'); store.save(); }}>
                    {label}
                  </button>
                ))}
              </span>
            </div>
            <button className="btn btn-danger btn-sm" onClick={() => store.resetProto()}>
              Reset prototype
            </button>
          </div>
        </div>
      )}
      {showProto && (
        <button
          onClick={() => store.setState({ demoOpen: !s.demoOpen })}
          data-register="charter"
          className="sim-proto-affordance"
          style={{ position: "fixed", right: 18, bottom: 18, zIndex: 70, fontFamily: "var(--font-mono)", fontSize: 9.5, letterSpacing: ".14em", textTransform: "uppercase", padding: "8px 14px", borderRadius: 999, border: "1px solid var(--line-2)", background: "#ffffff", color: "var(--tx-2)", cursor: "pointer", boxShadow: "var(--shadow-2)" }}
        >
          Prototype
        </button>
      )}
    </>
  );
}
