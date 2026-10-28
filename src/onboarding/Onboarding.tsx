import { useEffect, useRef, useState, type CSSProperties } from "react";
import marqueeBlack from "../assets/brand/citrate_marquee_black.svg";
import { LoaderMark } from "../components/LoaderMark";
import { TierPanel } from "./TierPanel";
import { PersonaPicker } from "../components/PersonaPicker";
import { bridge } from "../bridge";
import { AgentIdentityStep } from "./AgentIdentityStep";
import { ActivationTxPanel } from "./ActivationTxPanel";
import { Store } from "../shell/store";
import { AppState, fmtSaltFromWei } from "../shell/state";
import book from "../../src-tauri/addresses/40204.json";
import { useNetworkLabel } from "../shell/networkCompat";
import { CONSENSUS_LAG_NOTE, validatorWord, canOfferActivation, validatorDash, readStakeSalt } from "../shell/chainTruth";
import { GRANT_DISCLAIMER, grantExplanation } from "../data/grantCopy";
import { MEMBERSHIP_BUYS } from "../data/membership";
import { PricingLink } from "../components/PricingLink";
import { HaveAnInvite } from "../surfaces/HaveAnInvite";
import { SYNC_SOURCE, syncLine } from "../shell/syncProgress";
import { ACTIVATE_EXPLANATION, activationGate, activationGrantCopy, activationPhase, fmtDuration, keyMismatch, validatorState } from "./activateValidator";

const fmtI = (n: number) => Math.round(n).toLocaleString("en-US");
/** The registry stake as "N SALT", or "— (not read yet)" before a stakeOf read (rule 2). */
const fmtStake = (salt: number | null) => (salt == null ? "— (not read yet)" : `${fmtI(salt)} SALT`);
const short = (h: string) => (h ? h.slice(0, 6) + "…" + h.slice(-4) : "—");

const eyebrow: CSSProperties = {
  fontFamily: "var(--font-mono)",
  fontSize: 11,
  fontWeight: 500,
  letterSpacing: ".14em",
  textTransform: "uppercase",
  color: "var(--tx-3)",
};
const h1: CSSProperties = {
  fontFamily: "var(--font-display)",
  fontWeight: 420,
  fontSize: 32,
  lineHeight: 1.12,
  letterSpacing: "-0.011em",
};
const body: CSSProperties = { fontSize: 15, lineHeight: 1.6, color: "var(--tx-2)", margin: 0 };
const dataSrc: CSSProperties = {
  fontSize: 10,
  letterSpacing: ".1em",
  textTransform: "uppercase",
  color: "var(--tx-3)",
};
const check = (
  <svg viewBox="0 0 24 24" width="11" height="11" fill="none" stroke="currentColor" strokeWidth="2.6" strokeLinecap="round">
    <path d="M5 12 L10 17 L19 8"></path>
  </svg>
);
const checkBig = (w: number) => (
  <svg viewBox="0 0 24 24" width={w} height={w} fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round">
    <path d="M5 12 L10 17 L19 8"></path>
  </svg>
);

export function Onboarding({ store, s }: { store: Store; s: AppState }) {
  const stageOrder = ["s1", "s2", "s3", "s4", "s5", "s6", "s7"];
  const cur = stageOrder.indexOf(s.stage);
  const stages: [string, string][] = [
    ["s1", "Sign in"],
    ["s2", "Verify identity"],
    ["s3", "Free node or membership"],
    ["s4", "Wallet ready"],
    ["s5", "Membership grant"],
    ["s6", "Node ignition"],
    ["s7", "Activate validator"],
  ];

  // S0
  if (s.stage === "s0") {
    return (
      <div data-register="charter" style={{ height: "100vh", display: "flex", flexDirection: "column", background: "var(--srf-0)", overflow: "auto" }}>
        <div
          className="lattice-dots"
          style={{ flex: 1, display: "flex", flexDirection: "column", alignItems: "center", justifyContent: "center", gap: 26, padding: "48px 24px", textAlign: "center" }}
        >
          <div style={{ width: 200, height: 200 }}>
            <LoaderMark size={200} />
          </div>
          <img src={marqueeBlack} alt="Citrate" style={{ height: 20 }} />
          <div style={{ maxWidth: 580, display: "flex", flexDirection: "column", gap: 14, alignItems: "center" }}>
            <div style={{ fontFamily: "var(--font-display)", fontWeight: 380, fontSize: 46, lineHeight: 1.06, letterSpacing: "-0.022em", color: "var(--tx-1)" }}>
              The federation's house.
            </div>
            <p style={{ fontFamily: "var(--font-sans)", fontSize: 16, lineHeight: 1.55, color: "var(--tx-2)", margin: 0, maxWidth: 520 }}>
              Citrate runs a node, a wallet and Hermes on this machine. Your keys stay here.
            </p>
            <p style={{ fontFamily: "var(--font-sans)", fontSize: 14, lineHeight: 1.55, color: "var(--tx-2)", margin: 0, maxWidth: 520 }}>
              Running a node is free. Membership is optional, and you can choose it during setup or later.
            </p>
          </div>
          <div style={{ display: "flex", gap: 12, alignItems: "center" }}>
            <button className="btn btn-primary btn-lg" onClick={() => store.onJoin()}>
              Join the network
            </button>
            <button className="btn btn-ghost btn-lg" onClick={() => store.onExplore()}>
              Explore free
            </button>
          </div>
          {s.inviteLink && (
            <div style={{ width: "100%", maxWidth: 520, textAlign: "left" }}>
              <HaveAnInvite store={store} s={s} />
            </div>
          )}
          {/* Join 3 V-01 (fed#353): what Join involves, under the buttons. Owner decision 2026-10-27
              (option a): Join sets up a free node first, so membership and its bond are optional. */}
          <p style={{ fontFamily: "var(--font-sans)", fontSize: 13, lineHeight: 1.5, color: "var(--tx-3)", margin: 0, maxWidth: 520 }}>
            Membership and its bond are optional. You can upgrade later in Settings › Billing.
          </p>
          {/* Join 3 V-02: the network by its genesis-checked name (contract §2, §6), not the chain id. */}
          <NetworkName style={eyebrow} />
        </div>
      </div>
    );
  }

  return (
    <div data-register="charter" style={{ height: "100vh", display: "flex", flexDirection: "column", background: "var(--srf-0)", overflow: "auto" }}>
      <div style={{ flex: 1, display: "grid", gridTemplateColumns: "296px 1fr", minHeight: 0 }}>
        {/* progress spine */}
        <div style={{ borderRight: "1px solid var(--line-1)", background: "var(--srf-1)", padding: "26px 22px", display: "flex", flexDirection: "column", gap: 8, minHeight: 0, overflow: "auto" }}>
          <div style={{ display: "flex", alignItems: "center", marginBottom: 18 }}>
            {/* marquee-only, 2.5× (was the mark icon + a height:13 marquee) */}
            <img src={marqueeBlack} alt="Citrate" style={{ height: 33 }} />
          </div>
          <div style={{ ...eyebrow, marginBottom: 6 }}>Setup</div>
          {stages.map(([id, label], i) => {
            // The free path (no membership checkout, no grant) passes S5 without a grant: it is
            // marked skipped, never done.
            const skipped = id === "s5" && i < cur && s.s3 !== "settled" && !s.hasGrant;
            const done = i < cur && !skipped;
            const active = i === cur;
            return (
              <div key={label} style={{ display: "flex", alignItems: "center", gap: 12, padding: "9px 8px", borderRadius: "var(--r-1)" }}>
                <span
                  className="mono tabular"
                  style={{
                    width: 24,
                    height: 24,
                    borderRadius: 999,
                    display: "inline-flex",
                    alignItems: "center",
                    justifyContent: "center",
                    fontSize: 11,
                    background: done ? "var(--accent)" : active ? "var(--srf-2)" : "transparent",
                    color: done ? "#0e0f0c" : active ? "var(--tx-1)" : "var(--tx-3)",
                    border: "1px solid " + (done ? "var(--accent)" : active ? "var(--tx-1)" : "var(--line-2)"),
                    flexShrink: 0,
                  }}
                >
                  {done ? "✓" : skipped ? "–" : String(i + 1)}
                </span>
                <span style={{ flex: 1, fontSize: 13, fontWeight: active ? 500 : 400, color: active ? "var(--tx-1)" : done ? "var(--tx-2)" : "var(--tx-3)" }}>{label}</span>
                <span className="mono" style={{ fontSize: 9, letterSpacing: ".12em", textTransform: "uppercase", color: "var(--tx-3)" }}>
                  {skipped ? "skipped" : done ? "done" : active ? "now" : ""}
                </span>
              </div>
            );
          })}
          <div style={{ flex: 1 }}></div>
          <p style={{ fontSize: 12, lineHeight: 1.55, color: "var(--tx-3)", margin: 0, borderTop: "1px solid var(--line-1)", paddingTop: 14 }}>
            Every stage is resumable. Close the app at any point — it resumes at the first incomplete stage from server and chain truth.
          </p>
        </div>

        {/* stage content */}
        <div style={{ minHeight: 0, overflow: "auto", display: "flex", justifyContent: "center", padding: "56px 32px" }}>
          <div style={{ width: "100%", maxWidth: 620, display: "flex", flexDirection: "column", gap: 22 }}>
            <ClaimBanner store={store} s={s} />
            <OnboardingInvite store={store} s={s} />
            {s.stage === "s1" && <S1 store={store} s={s} />}
            {s.stage === "s2" && <S2 store={store} s={s} />}
            {s.stage === "s3" && <S3 store={store} s={s} />}
            {s.stage === "s4" && <S4 store={store} s={s} />}
            {s.stage === "s5" && <S5 store={store} s={s} />}
            {s.stage === "s6" && <S6 store={store} s={s} />}
            {s.stage === "s7" && <S7 store={store} s={s} />}
          </div>
        </div>
      </div>
    </div>
  );
}

function S1({ store, s }: { store: Store; s: AppState }) {
  const akLabels = [
    "device key created in the OS keyring",
    "hardware attestation proved — key never leaves this machine",
    "authority countersigned · device bound to your session",
  ];
  return (
    <div className="cc-fade-up" style={{ display: "flex", flexDirection: "column", gap: 20 }}>
      <div style={eyebrow}>S1 · Sign in</div>
      <div style={h1}>Sign in with your Citrate identity</div>
      <p style={body}>
        This app never shows credential fields. Your system browser opens auth.citrate.ai: passkey, email, Google, or sign-in-with-Ethereum. That is the anti-phishing
        posture, not a shortcut.
      </p>
      {s.s1 === "idle" && (
        <div>
          <button className="btn btn-primary btn-lg" onClick={() => store.onS1Start()}>
            Continue in your browser
          </button>
        </div>
      )}
      {s.s1 === "waiting" && (
        <div className="surface" style={{ display: "flex", alignItems: "center", gap: 18, padding: "18px 20px" }}>
          <div style={{ width: 46, height: 46, flexShrink: 0 }}>
            <LoaderMark size={46} />
          </div>
          <div style={{ display: "flex", flexDirection: "column", gap: 4, minWidth: 0 }}>
            <div style={{ fontSize: 14, fontWeight: 500 }}>Waiting for your browser…</div>
            <div className="mono" style={{ fontSize: 11, color: "var(--tx-3)" }}>
              auth.citrate.ai · loopback PKCE
            </div>
          </div>
          <div style={{ flex: 1 }}></div>
          <button className="btn btn-ghost btn-sm" onClick={() => store.onS1Cancel()}>
            Cancel
          </button>
        </div>
      )}
      {s.s1 === "attest" && (
        <div className="surface" style={{ display: "flex", flexDirection: "column", gap: 14, padding: "18px 20px" }}>
          <div style={{ display: "flex", alignItems: "center", gap: 16 }}>
            <div style={{ width: 46, height: 46, flexShrink: 0 }}>
              <LoaderMark size={46} />
            </div>
            <div style={{ display: "flex", flexDirection: "column", gap: 4 }}>
              <div style={{ fontSize: 14, fontWeight: 500 }}>Signed in — attesting this machine</div>
              <div className="mono" style={{ fontSize: 11, color: "var(--tx-3)" }}>
                a device proof binds your session and license to hardware you control
              </div>
            </div>
          </div>
          <div style={{ display: "flex", flexDirection: "column", gap: 8, borderTop: "1px solid var(--line-1)", paddingTop: 12 }}>
            {akLabels.map((label, i) => {
              const on = s.s1c > i;
              return (
                <div key={i} style={{ display: "flex", alignItems: "center", gap: 10 }}>
                  <span
                    style={{
                      width: 18,
                      height: 18,
                      borderRadius: 999,
                      display: "inline-flex",
                      alignItems: "center",
                      justifyContent: "center",
                      background: on ? "var(--ok-bg)" : "transparent",
                      border: "1px solid " + (on ? "var(--ok)" : "var(--line-2)"),
                      color: on ? "var(--ok)" : "transparent",
                      flexShrink: 0,
                    }}
                  >
                    {check}
                  </span>
                  <span className="mono" style={{ fontSize: 11.5, color: on ? "var(--tx-1)" : "var(--tx-3)" }}>
                    {label}
                  </span>
                </div>
              );
            })}
          </div>
        </div>
      )}
      {s.s1 === "done" && (
        <div className="surface" style={{ padding: "18px 20px", display: "flex", flexDirection: "column", gap: 12 }}>
          <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
            <span className="cc-stamp" style={{ width: 26, height: 26, borderRadius: 999, background: "var(--ok-bg)", border: "1px solid var(--ok)", color: "var(--ok)", display: "inline-flex", alignItems: "center", justifyContent: "center" }}>
              {checkBig(14)}
            </span>
            <div style={{ fontSize: 15, fontWeight: 500 }}>Signed in · machine attested</div>
          </div>
          <div style={{ display: "flex", flexDirection: "column", gap: 6, borderTop: "1px solid var(--line-1)", paddingTop: 10 }}>
            <Row k="Device" v={<span className="mono" style={{ fontSize: 12.5 }}>{s.deviceId} · this machine</span>} />
            <Row k="Proof" v={<span style={{ fontSize: 13 }}>hardware-backed key attestation, countersigned by the authority</span>} />
            <Row k="Binds" v={<span style={{ fontSize: 13 }}>session tokens and the license token ({"{sub, tier, exp, device_id}"}) to this device key</span>} />
          </div>
          <div>
            <button className="btn btn-primary" onClick={() => { store.setState({ stage: "s2" }); store.save(); }}>
              Continue to verification
            </button>
          </div>
        </div>
      )}
      <div className="mono" style={dataSrc}>
        Data source — citrate-identity OIDC · rotating refresh tokens in the OS keyring
      </div>
    </div>
  );
}

function Row({ k, v }: { k: string; v: React.ReactNode }) {
  return (
    <div style={{ display: "flex", gap: 10, alignItems: "baseline" }}>
      <span className="mono" style={{ fontSize: 10, letterSpacing: ".1em", textTransform: "uppercase", color: "var(--tx-3)", width: 110, flexShrink: 0 }}>
        {k}
      </span>
      {v}
    </div>
  );
}

/**
 * PHONEPAY-S4 — the claim banner. Rendered above every onboarding stage when a
 * `citrate://claim` link is pending. It (a) tells the buyer which account they paid
 * with and (b) hard-flags a wrong-account mismatch (paid as X, signed in as Y) with a
 * one-tap switch. It is DISPLAY ONLY — it grants nothing; the authority sign-in and the
 * server-side sub↔order reconciler are the sole gates (A8/A2).
 */
// D-12 — an invite link that arrived (deep link) or was pasted waits here through setup, on the same
// component as Groups. Before the wallet step the Join button says the request goes once setup gets
// there; from then on Join sends the request. Never a silent join: the inviter approves it.
function OnboardingInvite({ store, s }: { store: Store; s: AppState }) {
  return (
    <div className="surface" style={{ padding: "10px 14px" }} data-testid="onboarding-invite">
      <HaveAnInvite store={store} s={s} />
    </div>
  );
}

function ClaimBanner({ store, s }: { store: Store; s: AppState }) {
  const claim = s.pendingClaim;
  if (!claim?.email) return null;
  const mismatch = store.claimAccountMismatch();
  if (mismatch) {
    return (
      <div className="cc-fade-up" style={{ border: "1px solid var(--danger)", background: "var(--danger-bg)", borderRadius: "var(--r-2)", padding: "14px 16px", display: "flex", flexDirection: "column", gap: 8 }}>
        <div style={{ fontSize: 14, fontWeight: 600, color: "var(--danger)" }}>Wrong account for this membership</div>
        <p style={{ fontSize: 13, lineHeight: 1.55, color: "var(--tx-1)", margin: 0 }}>
          This membership was purchased as <strong>{mismatch.paidAs}</strong>, but you&rsquo;re signed in as <strong>{mismatch.signedInAs}</strong>. Sign in with the account you paid with so your seat binds to the right identity. Nothing here grants access — the authority is the gate.
        </p>
        <div>
          <button
            className="btn btn-primary btn-sm"
            onClick={() => {
              store.setState({ stage: "s1", s1: "idle" });
              store.save();
              void store.authLogout();
            }}
          >
            Sign out &amp; switch account
          </button>
        </div>
      </div>
    );
  }
  const matched = s.signedIn && (s.authEmail ?? "").trim().toLowerCase() === claim.email.trim().toLowerCase();
  return (
    <div className="cc-fade-up surface" style={{ padding: "14px 16px", display: "flex", flexDirection: "column", gap: 6 }}>
      <div style={{ fontSize: 13.5, fontWeight: 500 }}>{matched ? "Claiming your membership" : "Claim your membership"}</div>
      <p style={{ fontSize: 12.5, lineHeight: 1.55, color: "var(--tx-2)", margin: 0 }}>
        {matched ? (
          <>
            You paid as <strong>{claim.email}</strong>. Your seat (the membership SBT, and a 32,000 SALT grant locked in your bond) lands on chain once the grant is
            issued — it will appear here. {GRANT_DISCLAIMER}
          </>
        ) : (
          <>
            Paid as <strong>{claim.email}</strong>. Sign in with that account so your membership binds to the right identity.
          </>
        )}
      </p>
    </div>
  );
}

function S2({ store, s }: { store: Store; s: AppState }) {
  return (
    <div className="cc-fade-up" style={{ display: "flex", flexDirection: "column", gap: 20 }}>
      <div style={eyebrow}>S2 · Verify</div>
      <div style={h1}>Verify your identity</div>
      <p style={body}>
        Verification is optional — you can finish it any time. Running a node doesn&apos;t need it, and neither does membership; identity verification unlocks certain features and upgrades you
        to the <span className="mono">commercial.kyc</span> tier. It runs in your browser with our in-house provider; this app only observes the claim change, and your
        documents never touch this machine.
      </p>
      {s.s2 === "none" && (
        <div>
          <button className="btn btn-primary btn-lg" onClick={() => store.onS2Start()}>
            Start verification
          </button>
        </div>
      )}
      {s.s2 === "pending" && (
        <div className="surface" style={{ display: "flex", alignItems: "center", gap: 18, padding: "18px 20px" }}>
          <div style={{ width: 46, height: 46, flexShrink: 0 }}>
            <LoaderMark size={46} />
          </div>
          <div style={{ display: "flex", flexDirection: "column", gap: 4 }}>
            <div style={{ fontSize: 14, fontWeight: 500 }}>Verification in progress</div>
            <div className="mono" style={{ fontSize: 11, color: "var(--tx-3)" }}>
              polling /kyc/status · every 5s
            </div>
          </div>
        </div>
      )}
      {s.s2 === "verified" && (
        <div className="surface" style={{ padding: "18px 20px", display: "flex", flexDirection: "column", gap: 12 }}>
          <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
            <span className="cc-stamp" style={{ width: 26, height: 26, borderRadius: 999, background: "var(--ok-bg)", border: "1px solid var(--ok)", color: "var(--ok)", display: "inline-flex", alignItems: "center", justifyContent: "center" }}>
              {checkBig(14)}
            </span>
            <div style={{ fontSize: 15, fontWeight: 500 }}>Identity verified</div>
          </div>
          <div className="mono" style={{ fontSize: 11, color: "var(--tx-2)" }}>
            kyc_status: verified · entitlement +commercial.kyc granted by the authority
          </div>
          <div>
            <button className="btn btn-primary" onClick={() => { store.setState({ stage: "s3" }); store.save(); }}>
              Continue
            </button>
          </div>
        </div>
      )}
      {s.s2 === "failed" && (
        <div style={{ border: "1px solid var(--danger)", background: "var(--danger-bg)", borderRadius: "var(--r-2)", padding: "16px 18px", display: "flex", flexDirection: "column", gap: 10 }}>
          <div style={{ fontSize: 14, fontWeight: 500, color: "var(--danger)" }}>Verification failed</div>
          <p style={{ fontSize: 13, lineHeight: 1.55, color: "var(--tx-2)", margin: 0 }}>
            The document check did not pass. You can retry with different documents — nothing was stored on this machine.
          </p>
          <div>
            <button className="btn btn-ghost btn-sm" onClick={() => store.onS2Start()}>
              Retry verification
            </button>
          </div>
        </div>
      )}
      {s.s2 === "review" && (
        <div style={{ border: "1px solid var(--warn)", background: "var(--warn-bg)", borderRadius: "var(--r-2)", padding: "16px 18px", display: "flex", flexDirection: "column", gap: 10 }}>
          <div style={{ fontSize: 14, fontWeight: 500, color: "var(--warn)" }}>Manual review</div>
          <p style={{ fontSize: 13, lineHeight: 1.55, color: "var(--tx-2)", margin: 0 }}>
            Your verification needs a human look — usually within one business day. You'll get an email, and your tier upgrades to <span className="mono">commercial.kyc</span> on
            its own when the claim changes. You don't have to wait — continue below and finish verification at your convenience.
          </p>
        </div>
      )}
      {s.s2 !== "verified" && (
        <div style={{ borderTop: "1px solid var(--line-2)", paddingTop: 16, display: "flex", flexDirection: "column", gap: 8 }}>
          <button className="btn btn-primary btn-lg" onClick={() => { store.setState({ stage: "s3" }); store.save(); }}>
            Skip for now — continue
          </button>
          <p className="mono" style={{ fontSize: 11, color: "var(--tx-3)", margin: 0 }}>
            Running a node and membership don&apos;t need it. You keep the <span className="mono">commercial</span> tier now and can finish KYC any time to upgrade to
            {" "}<span className="mono">commercial.kyc</span>.
          </p>
        </div>
      )}
      <div className="mono" style={dataSrc}>
        Data source — citrate-identity /kyc/start · /kyc/status · /userinfo
      </div>
    </div>
  );
}

/** Enterprise · Contact us — a qualifying lead form (built to convert + prep the sales call). Org +
 *  work email are required; the rest help sales. Submits to core-membership via
 *  store.submitEnterpriseLead (server-side validate + encrypt + audit). Honest states only (Rule 1). */
function EnterpriseContactForm({ store, onDone }: { store: Store; onDone: () => void }) {
  const [f, setF] = useState({ org: "", email: "", contact: "", seats: "", workload: "", timeline: "", notes: "" });
  const [st, setSt] = useState<{ kind: "idle" | "sending" | "done" | "error"; msg?: string }>({ kind: "idle" });
  const validEmail = /^[^@\s]+@[^@\s]+\.[^@\s]+$/.test(f.email.trim());
  const canSend = f.org.trim().length > 0 && validEmail && st.kind !== "sending";
  const inputStyle: CSSProperties = {
    width: "100%", padding: "9px 11px", fontSize: 13, borderRadius: "var(--r-1, 6px)",
    border: "1px solid var(--line-1)", background: "var(--srf-2)", color: "var(--tx-1)",
  };
  const labelStyle: CSSProperties = { fontSize: 11.5, color: "var(--tx-2)", marginBottom: 4, display: "block" };

  async function send() {
    setSt({ kind: "sending" });
    const res = await store.submitEnterpriseLead({
      org: f.org.trim(),
      email: f.email.trim(),
      contact: f.contact.trim() || undefined,
      seats: f.seats.trim() || undefined,
      workload: f.workload || undefined,
      timeline: f.timeline || undefined,
      notes: f.notes.trim() || undefined,
    });
    if (res.ok) setSt({ kind: "done" });
    else setSt({ kind: "error", msg: res.error });
  }

  if (st.kind === "done") {
    return (
      <div className="surface" style={{ padding: "18px 20px", display: "flex", flexDirection: "column", gap: 10 }}>
        <div style={{ fontFamily: "var(--font-display)", fontSize: 17 }}>Thanks — your request is in.</div>
        <p style={{ fontSize: 13, color: "var(--tx-2)", margin: 0, lineHeight: 1.55 }}>
          Our team will reach out to <strong>{f.email.trim()}</strong> about {f.org.trim()} — seats, topology, compliance, and volume pricing. You can start on the Free tier in the meantime.
        </p>
        <div style={{ display: "flex", gap: 8, marginTop: 2 }}>
          <button className="btn btn-primary btn-sm" onClick={() => store.onS3Free()}>Continue free for now</button>
          <button className="btn btn-ghost btn-sm" onClick={onDone}>Close</button>
        </div>
      </div>
    );
  }

  return (
    <div className="surface" style={{ padding: "18px 20px", display: "flex", flexDirection: "column", gap: 12 }}>
      <div style={eyebrow}>Enterprise · tell us what you need</div>
      <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 12 }}>
        <div>
          <label style={labelStyle}>Organization *</label>
          <input style={inputStyle} value={f.org} placeholder="Acme Corp" onChange={(e) => { const v = e.currentTarget.value; setF((p) => ({ ...p, org: v })); }} />
        </div>
        <div>
          <label style={labelStyle}>Work email *</label>
          <input style={inputStyle} value={f.email} placeholder="you@acme.com" onChange={(e) => { const v = e.currentTarget.value; setF((p) => ({ ...p, email: v })); }} />
        </div>
        <div>
          <label style={labelStyle}>Your name</label>
          <input style={inputStyle} value={f.contact} placeholder="Dana Okafor" onChange={(e) => { const v = e.currentTarget.value; setF((p) => ({ ...p, contact: v })); }} />
        </div>
        <div>
          <label style={labelStyle}>Seats / nodes</label>
          <input style={inputStyle} value={f.seats} placeholder="e.g. 50 or 100+" onChange={(e) => { const v = e.currentTarget.value; setF((p) => ({ ...p, seats: v })); }} />
        </div>
        <div>
          <label style={labelStyle}>Primary workload</label>
          <select style={inputStyle} value={f.workload} onChange={(e) => { const v = e.currentTarget.value; setF((p) => ({ ...p, workload: v })); }}>
            <option value="">Select…</option>
            <option value="storage">Data storage</option>
            <option value="training">Model training</option>
            <option value="inference">Inference</option>
            <option value="apps">Building apps</option>
            <option value="mixed">Mixed</option>
          </select>
        </div>
        <div>
          <label style={labelStyle}>Timeline</label>
          <select style={inputStyle} value={f.timeline} onChange={(e) => { const v = e.currentTarget.value; setF((p) => ({ ...p, timeline: v })); }}>
            <option value="">Select…</option>
            <option value="evaluating">Just evaluating</option>
            <option value="1-3mo">1–3 months</option>
            <option value="now">Ready now</option>
          </select>
        </div>
      </div>
      <div>
        <label style={labelStyle}>Anything else? (compliance needs, topology, use case)</label>
        <textarea style={{ ...inputStyle, minHeight: 66, resize: "vertical" }} value={f.notes} onChange={(e) => { const v = e.currentTarget.value; setF((p) => ({ ...p, notes: v })); }} />
      </div>
      {st.kind === "error" && (
        <p role="alert" style={{ fontSize: 12.5, color: "var(--danger, #dd7259)", margin: 0 }}>{st.msg || "Could not send your request. Please try again."}</p>
      )}
      <div style={{ display: "flex", gap: 10, alignItems: "center" }}>
        <button className="btn btn-primary btn-sm" disabled={!canSend} onClick={send}>
          {st.kind === "sending" ? "Sending…" : "Send request"}
        </button>
        <button className="btn btn-ghost btn-sm" onClick={onDone}>Cancel</button>
        <span style={{ fontSize: 11, color: "var(--tx-3)", marginLeft: "auto" }}>Your details are encrypted. No card, no charge.</span>
      </div>
    </div>
  );
}

// Owner decision 2026-10-27 (option a): S3 opens on a choice, with the free node first. Running a
// node needs no payment; membership is optional. The price shows only inside MembershipCheckout,
// which mounts only after the member chooses it (or resumes a checkout already started).
export function S3({ store, s }: { store: Store; s: AppState }) {
  const [member, setMember] = useState(false);
  if (member || s.s3 !== "idle") {
    // Back also ends a running pre-checkout grant check, so it can open no checkout later (core#584).
    const back = () => {
      store.endS3Check();
      setMember(false);
    };
    return <MembershipCheckout store={store} s={s} onBack={s.s3 === "idle" ? back : undefined} />;
  }
  const freeGets = [
    "A full node on this machine: it follows the chain, checks every block and serves your reads locally",
    "Your wallet: a signing key sealed on this device",
    "Hermes, chain reads and the marketplace view",
  ];
  return (
    <div className="cc-fade-up" style={{ display: "flex", flexDirection: "column", gap: 20 }}>
      <div style={eyebrow}>S3 · Free node or membership</div>
      <div style={h1}>Run your node for free</div>
      <p style={body}>
        You don&apos;t need a membership to run a node, use your wallet or work with Hermes. Membership is optional: choose it now, or later in Settings › Billing.
      </p>
      <div className="surface" data-testid="s3-free-path" style={{ padding: "18px 20px", display: "flex", flexDirection: "column", gap: 10, borderColor: "var(--tx-1)" }}>
        <div style={eyebrow}>Free · no charge</div>
        {freeGets.map((text, i) => (
          <div key={i} style={{ display: "flex", gap: 10, alignItems: "baseline" }}>
            <span style={{ width: 5, height: 5, borderRadius: 999, background: "var(--accent)", flexShrink: 0, position: "relative", top: -2 }}></span>
            <span style={{ fontSize: 13.5, lineHeight: 1.5, color: "var(--tx-1)" }}>{text}</span>
          </div>
        ))}
        <p style={{ fontSize: 12, lineHeight: 1.5, color: "var(--tx-3)", margin: 0 }}>
          No membership, no bond. Your node produces blocks only if it becomes an active validator.
        </p>
        <div>
          <button className="btn btn-primary btn-lg" onClick={() => store.onS3Free()}>
            Run a node for free
          </button>
        </div>
      </div>
      <div className="surface" data-testid="s3-member-path" style={{ padding: "18px 20px", display: "flex", flexDirection: "column", gap: 10 }}>
        <div style={eyebrow}>Membership · optional</div>
        <p style={{ fontSize: 13, lineHeight: 1.55, color: "var(--tx-2)", margin: 0 }}>
          Adds the membership SBT (the on-chain record of your seat), member-tier features across the app and Commissary access at your tier. The next screen
          shows what it includes and its price. Nothing is charged until you check out in your browser.
        </p>
        <div>
          <button className="btn btn-ghost" onClick={() => setMember(true)}>
            Become a member (optional)
          </button>
        </div>
      </div>
      <div className="mono" style={dataSrc}>
        Free path — no payment and no checkout · membership checkout — core-membership, in your browser
      </div>
    </div>
  );
}

/** The member checkout: the only onboarding screen that shows the membership price. Exported so
 *  tests (and #349/#379's money-copy tests) can render the checkout directly. */
export function MembershipCheckout({ store, s, onBack }: { store: Store; s: AppState; onBack?: () => void }) {
  const [entOpen, setEntOpen] = useState(false);
  // #31 (Luke 2026-09-11) — auto-open checkout once the wallet is LINKED, so the member
  // doesn't hunt for a second button. This component mounts only after the member chose
  // membership on S3, so landing on S3 never opens checkout by itself. The "Link your wallet" click is the explicit intent
  // signal (or the wallet was linked in a prior session); once linked + idle on S3 we open
  // the Stripe checkout automatically. The settle→S4 effect below then finishes the flow
  // when the grant poll lands (the approved "auto-open + auto-advance on poll" shape).
  // Fires ONCE (ref-guarded); onS3Pay itself still enforces the link-first ordering guard,
  // and the tier cards + "Continue free" stay, so the browser can be closed without paying.
  const autoCheckoutRef = useRef(false);
  useEffect(() => {
    if (s.stage === "s3" && s.s3 === "idle" && store.walletIsLinked() && !autoCheckoutRef.current) {
      autoCheckoutRef.current = true;
      store.onS3Pay();
    }
  }, [s.stage, s.s3, s.walletAddr, s.custodyAddr, store]);
  // PHONEPAY-S10 — auto-advance to S4 once the grant settles, so there is no extra
  // "Continue" click after checkout. We hold ~1.2s so the "Payment settled" stamp is
  // seen, then move on. The manual button below stays as a fallback (e.g. if a
  // background timer is throttled). Guarded on stage so it fires only while S3 is live.
  useEffect(() => {
    if (s.s3 === "settled" && s.stage === "s3") {
      const t = setTimeout(() => {
        store.setState({ stage: "s4" });
        store.save();
      }, 1200);
      return () => clearTimeout(t);
    }
  }, [s.s3, s.stage, store]);
  const items = [
    "Tier features across the app — chat on the gateway, docs at member tier, gated downloads",
    "A 32,000 SALT grant, locked in your bond — it becomes your validator stake when you activate the bond",
    "Membership SBT — the non-transferable on-chain record of your seat",
    "Commissary access — apps, SDKs, docs, and services at your tier",
  ];
  return (
    <div className="cc-fade-up" style={{ display: "flex", flexDirection: "column", gap: 20 }}>
      <div style={eyebrow}>S3 · Membership</div>
      <div style={h1}>Yearly membership</div>
      {onBack && (
        <div>
          <button className="btn btn-ghost btn-sm" onClick={onBack}>
            ← Back to the free node
          </button>
        </div>
      )}
      <div style={{ display: "grid", gridTemplateColumns: "1fr 1.2fr 1fr", gap: 12 }}>
        <div style={{ border: "1px solid var(--line-1)", background: "var(--srf-1)", borderRadius: "var(--r-2)", padding: 16, display: "flex", flexDirection: "column", gap: 6 }}>
          <div style={eyebrow}>Free</div>
          <div style={{ fontFamily: "var(--font-display)", fontSize: 24, fontWeight: 420 }}>No charge</div>
          <p style={{ fontSize: 12, lineHeight: 1.5, color: "var(--tx-2)", margin: 0, flex: 1 }}>Wallet, agent harness, chain reads, marketplace view. No staking, no grant.</p>
          {s.s3 === "idle" && (
            <button className="btn btn-ghost btn-sm" style={{ marginTop: 8, alignSelf: "flex-start" }} disabled={s.s3Checking} onClick={() => store.onS3Free()}>
              Continue free →
            </button>
          )}
        </div>
        <div style={{ border: "2px solid var(--tx-1)", background: "var(--srf-2)", borderRadius: "var(--r-2)", padding: 16, display: "flex", flexDirection: "column", gap: 6, position: "relative" }}>
          <div style={{ ...eyebrow, color: "var(--accent-text)" }}>Pilot · selected</div>
          <div style={{ fontFamily: "var(--font-display)", fontSize: 24, fontWeight: 420 }}>
            {/* C1-V V12: no in-app price until core-membership serves one (chain-team ruling). */}
            <PricingLink open={(u) => void store.openExternal(u)} style={{ fontSize: 15 }} />
          </div>
          <p style={{ fontSize: 12, lineHeight: 1.5, color: "var(--tx-2)", margin: 0 }}>Full membership. Everything listed below.</p>
        </div>
        <div style={{ border: "1px solid var(--line-1)", background: "var(--srf-1)", borderRadius: "var(--r-2)", padding: 16, display: "flex", flexDirection: "column", gap: 6 }}>
          <div style={eyebrow}>Enterprise</div>
          <div style={{ fontFamily: "var(--font-display)", fontSize: 24, fontWeight: 420 }}>Custom</div>
          <p style={{ fontSize: 12, lineHeight: 1.5, color: "var(--tx-2)", margin: 0, flex: 1 }}>Org seats, on-prem topology, custom compliance plane, volume pricing.</p>
          {s.s3 === "idle" && (
            <button className="btn btn-ghost btn-sm" style={{ marginTop: 8, alignSelf: "flex-start" }} onClick={() => setEntOpen((v) => !v)}>
              {entOpen ? "Close" : "Contact sales →"}
            </button>
          )}
        </div>
      </div>
      {entOpen && s.s3 === "idle" && <EnterpriseContactForm store={store} onDone={() => setEntOpen(false)} />}
      <div className="surface" style={{ padding: "18px 20px", display: "flex", flexDirection: "column", gap: 10 }}>
        <div style={eyebrow}>What membership carries</div>
        {items.map((text, i) => (
          <div key={i} style={{ display: "flex", gap: 10, alignItems: "baseline" }}>
            <span style={{ width: 5, height: 5, borderRadius: 999, background: "var(--accent)", flexShrink: 0, position: "relative", top: -2 }}></span>
            <span style={{ fontSize: 13.5, lineHeight: 1.5, color: "var(--tx-1)" }}>{text}</span>
          </div>
        ))}
        <p style={{ fontSize: 11.5, lineHeight: 1.5, color: "var(--tx-3)", margin: "6px 0 0" }}>
          {MEMBERSHIP_BUYS} {grantExplanation({ amountSalt: "32,000", unlockBlock: null, staked: false })}
        </p>
      </div>
      {s.s3 === "idle" && (
        <div>
          {!store.walletIsLinked() ? (
            <>
              <button className="btn btn-primary btn-lg" onClick={() => store.linkWallet()}>
                Link your wallet
              </button>
              <p style={{ fontSize: 12, lineHeight: 1.5, color: "var(--tx-3)", margin: "8px 0 0" }}>
                Binds this device's wallet to your identity so your membership funds an address only you can spend from. Approve the link, then check out.
              </p>
            </>
          ) : (
            <>
              {/* Disabled while the pre-checkout grant check runs: one attempt at a time (core#584). */}
              <button className="btn btn-primary btn-lg" disabled={s.s3Checking} onClick={() => store.onS3Pay()}>
                Check out in your browser
              </button>
              {s.s3Checking && (
                <p className="mono" style={{ fontSize: 11, color: "var(--tx-3)", margin: "8px 0 0" }}>
                  Checking chain 40204 for a membership this account already holds…
                </p>
              )}
            </>
          )}
        </div>
      )}
      {s.s3 === "paying" && !s.s3PollExhausted && (
        <div className="surface" style={{ display: "flex", alignItems: "center", gap: 18, padding: "18px 20px" }}>
          <div style={{ width: 46, height: 46, flexShrink: 0 }}>
            <LoaderMark size={46} />
          </div>
          <div style={{ display: "flex", flexDirection: "column", gap: 4 }}>
            <div style={{ fontSize: 14, fontWeight: 500 }}>Waiting for checkout…</div>
            <div className="mono" style={{ fontSize: 11, color: "var(--tx-3)" }}>
              core-membership · settlement confirmed by webhook, not by this app
            </div>
          </div>
        </div>
      )}
      {/* The poll ran out of budget with no grant on chain. This is NOT an error and
          NOT a failed payment — the membership grant is provisioned server-side and
          can legitimately land later (the reconciler re-drives stranded grants on a
          10-minute cron, with a 6-hour backoff after a denial). Say exactly that, and
          offer a re-check that only READS the chain. Deliberately no "pay again"
          affordance here: the payment already settled, and a second checkout would
          charge again for a membership that is already owed. */}
      {s.s3 === "paying" && s.s3PollExhausted && (
        <div className="surface" style={{ padding: "18px 20px", display: "flex", flexDirection: "column", gap: 12 }} data-testid="s3-poll-exhausted">
          <div style={{ fontSize: 15, fontWeight: 500 }}>Payment received — the grant has not landed yet</div>
          <p style={{ fontSize: 13, lineHeight: 1.55, color: "var(--tx-2)", margin: 0 }}>
            Your payment settled. The membership grant (the SBT, and a 32,000 SALT grant locked in your bond) is
            issued server-side and has not appeared on chain yet. {GRANT_DISCLAIMER} This app stopped polling to
            avoid hammering the network — it does not mean anything failed, and you do not need
            to pay again.
          </p>
          <div style={{ display: "flex", alignItems: "center", gap: 12 }}>
            <button className="btn btn-primary" onClick={() => store.recheckMembership()}>
              Check again
            </button>
            <span className="mono" style={{ fontSize: 11, color: "var(--tx-3)" }}>
              reads chain 40204 · never re-opens checkout
            </span>
          </div>
          <p style={{ fontSize: 11.5, lineHeight: 1.5, color: "var(--tx-3)", margin: 0 }}>
            Still nothing after a while? Your seat is recorded and owed — contact support with
            your order rather than paying twice.
          </p>
        </div>
      )}
      {s.s3 === "settled" && (
        <div className="surface" style={{ padding: "18px 20px", display: "flex", alignItems: "center", gap: 14 }}>
          <span className="cc-stamp" style={{ width: 26, height: 26, borderRadius: 999, background: "var(--ok-bg)", border: "1px solid var(--ok)", color: "var(--ok)", display: "inline-flex", alignItems: "center", justifyContent: "center" }}>
            {checkBig(14)}
          </span>
          <div style={{ flex: 1 }}>
            <div style={{ fontSize: 15, fontWeight: 500 }}>Payment settled</div>
            {/* Q-A.4b item 9 — the fabricated static order id (ord_2026_84117) is
                dropped: the entitlement/userinfo claim exposes no real order id, so
                showing one would be an invented identifier (Rule 1). The settlement
                is confirmed by the webhook-driven entitlement, not a UI id. */}
            <div className="mono" style={{ fontSize: 11, color: "var(--tx-3)" }}>
              settlement confirmed by webhook · idempotent · audit-logged
            </div>
          </div>
          <button className="btn btn-primary" onClick={() => { store.setState({ stage: "s4" }); store.save(); }}>
            Continue
          </button>
        </div>
      )}
    </div>
  );
}

export function S4({ store, s }: { store: Store; s: AppState }) {
  // core#425 — the wallet in 0.5.2 is an EVM signing key sealed in this device's custody vault
  // (custodyAddr). There is no smart account, passkey signer, guardian or recovery service, and
  // losing the device loses the wallet (O-21). The card shows the device key's own address, never
  // the identity's wallet_address claim: until the two are linked that claim is a different address
  // this key cannot sign for. In the web-dev persona path (signedIn=false) the labelled persona
  // address stands in, as before.
  const deviceKey = s.custodyAddr || (!s.signedIn ? s.walletAddr : "");
  const linkNote = deviceKey === "" ? "device key not read yet" : store.walletIsLinked() ? "linked to your identity" : "not linked to your identity yet";
  const member = s.s3 === "settled" || s.hasGrant;
  return (
    <div className="cc-fade-up" style={{ display: "flex", flexDirection: "column", gap: 20 }}>
      <div style={eyebrow}>S4 · Wallet ready</div>
      <div style={h1}>Your wallet is a key on this device</div>
      <p style={body}>
        Citrate Core made a signing key for you and sealed it in this computer&apos;s keychain. It signs only when you approve a request on screen. This version has no
        recovery for it: if this computer or its keychain is lost, the wallet and everything in it is lost with it.
      </p>
      <div className="surface" style={{ padding: "18px 20px", display: "flex", flexDirection: "column", gap: 12 }}>
        <div className="lbl">Wallet key on this device</div>
        <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
          <span className="mono" style={{ fontSize: 14, wordBreak: "break-all" }}>{deviceKey || "—"}</span>
          {deviceKey && (
            <button className="btn btn-ghost btn-sm" onClick={() => store.copy(deviceKey, "Address copied")}>
              Copy
            </button>
          )}
        </div>
        <div style={{ borderTop: "1px solid var(--line-1)", paddingTop: 12, display: "flex", flexDirection: "column", gap: 8 }}>
          <Row k="Key" v={<span style={{ fontSize: 13 }}>EVM signing key · sealed in the custody vault · {linkNote}</span>} />
          <Row k="Signing" v={<span style={{ fontSize: 13 }}>Only through the signature ceremony, after you approve</span>} />
          <Row k="Recovery" v={<span style={{ fontSize: 13 }}>No recovery in this version: device loss is wallet loss</span>} />
        </div>
      </div>
      <div>
        {/* The free path (no membership checkout, no grant) has no grant step: straight to the node. */}
        <button className="btn btn-primary btn-lg" onClick={() => { store.setState({ stage: member ? "s5" : "s6" }); store.save(); }}>
          {member ? "Continue to the grant ceremony" : "Continue to your node"}
        </button>
      </div>
      <div className="mono" style={dataSrc}>
        Data source — device key address (custody vault) · linked when your identity&apos;s wallet_address claim is the same address
      </div>
    </div>
  );
}

export function S5({ store, s }: { store: Store; s: AppState }) {
  // Grant gate (ADR-2026-07-25): sub + settled payment. KYC is not a required
  // proof here — it picks the entitlement tier (commercial vs commercial.kyc).
  const ckLabels = ["authenticated sub — token live", "payment settled — webhook proof", "entitlement raised — tier set by KYC state"];
  return (
    <div className="cc-fade-up" style={{ display: "flex", flexDirection: "column", gap: 20 }}>
      <div style={eyebrow}>S5 · Membership grant</div>
      {s.s5 === "idle" && (
        <div style={{ display: "flex", flexDirection: "column", gap: 20 }}>
          <div style={h1}>Your 32,000 SALT membership grant</div>
          <p style={body}>
            {/* C1-V V5: the one grant-custody explanation (UI truth contract §5.4). */}
            {grantExplanation({ amountSalt: "32,000", unlockBlock: null, staked: false })}
          </p>
          <div>
            <button className="btn btn-primary btn-lg" onClick={() => store.onS5Begin()}>
              Begin the ceremony
            </button>
          </div>
        </div>
      )}
      {s.s5 === "verifying" && (
        <div style={{ display: "flex", flexDirection: "column", alignItems: "center", gap: 22, padding: "16px 0" }}>
          <div style={{ width: 150, height: 150 }}>
            <LoaderMark size={150} />
          </div>
          <div style={eyebrow}>Verifying the three proofs</div>
          <div style={{ display: "flex", flexDirection: "column", gap: 10, width: "100%", maxWidth: 380 }}>
            {ckLabels.map((label, i) => {
              const on = s.s5c > i;
              return (
                <div key={i} style={{ display: "flex", alignItems: "center", gap: 10 }}>
                  <span
                    style={{
                      width: 20,
                      height: 20,
                      borderRadius: 999,
                      display: "inline-flex",
                      alignItems: "center",
                      justifyContent: "center",
                      background: on ? "var(--ok-bg)" : "transparent",
                      border: "1px solid " + (on ? "var(--ok)" : "var(--line-2)"),
                      color: on ? "var(--ok)" : "transparent",
                      flexShrink: 0,
                    }}
                  >
                    <svg viewBox="0 0 24 24" width="12" height="12" fill="none" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round">
                      <path d="M5 12 L10 17 L19 8"></path>
                    </svg>
                  </span>
                  <span className="mono" style={{ fontSize: 12, color: on ? "var(--tx-1)" : "var(--tx-3)" }}>
                    {label}
                  </span>
                </div>
              );
            })}
          </div>
          <div className="mono" style={dataSrc}>
            re-verified server-side · any missing proof aborts with no partial grant
          </div>
        </div>
      )}
      {s.s5 === "settling" && (
        <div style={{ display: "flex", flexDirection: "column", alignItems: "center", gap: 14, padding: "26px 0" }}>
          <div style={eyebrow}>Arriving · locking in your bond</div>
          <div className="tabular" style={{ fontFamily: "var(--font-display)", fontWeight: 380, fontSize: 72, lineHeight: 1, letterSpacing: "-0.022em", color: "var(--tx-1)" }}>
            {fmtI(s.s5n)}
          </div>
          <div style={{ ...eyebrow, color: "var(--accent-text)" }}>SALT → MembershipStakeVault → your MemberBond</div>
          <div className="mono" style={{ fontSize: 11, color: "var(--tx-3)" }}>
            sent by the membership treasury, not by this app
          </div>
        </div>
      )}
      {s.s5 === "settled" && (
        <div style={{ display: "flex", flexDirection: "column", gap: 16 }}>
          <div className="surface cc-stamp" style={{ padding: 22, display: "flex", flexDirection: "column", gap: 14, borderColor: "var(--accent)" }}>
            <div style={{ display: "flex", alignItems: "baseline", gap: 12 }}>
              <div className="tabular" style={{ fontFamily: "var(--font-display)", fontWeight: 380, fontSize: 44, lineHeight: 1, letterSpacing: "-0.022em" }}>
                {fmtSaltFromWei(s.s5StakeWei)}
              </div>
              <div style={{ ...eyebrow, color: "var(--accent-text)" }}>SALT grant · locked in your bond</div>
            </div>
            <div style={{ display: "flex", flexDirection: "column", gap: 8, borderTop: "1px solid var(--line-1)", paddingTop: 12 }}>
              {/* F1 (Rule 1): every row traces to a real read. The grant figure is the
                  REAL attributedStake (wei→SALT), locked in the bond and not stake (C1-V V5); the SBT row shows only "minted"
                  (confirmed by balanceOf==1 — the token id was never read); the
                  honest on-chain anchor is the member/vault address, NOT a fabricated
                  tx hash (this app does not broadcast the grant tx). */}
              <SettleRow k="Grant" v={`${fmtSaltFromWei(s.s5StakeWei)} SALT · ${s.s5BondStatus ?? "locked in your bond"}`} />
              <SettleRow k="Membership SBT" v="CitrateMemberSBT · minted" />
              <SettleRow k="Member wallet" v={short(s.walletAddr)} accent />
            </div>
            <p style={{ fontSize: 12, lineHeight: 1.55, color: "var(--tx-3)", margin: 0 }}>
              {grantExplanation({ amountSalt: fmtSaltFromWei(s.s5StakeWei), unlockBlock: s.s5UnlockBlock, staked: s.bondedStake > 0, granted: s.hasGrant, activated: s.bondActivated })}
            </p>
          </div>
          <div>
            <button className="btn btn-primary btn-lg" onClick={() => { store.setState({ stage: "s6" }); store.save(); }}>
              Continue to node ignition
            </button>
          </div>
        </div>
      )}
      <div className="mono" style={dataSrc}>
        {/* C1-V V9: addresses come from the bundled address book (a reroll moves them). */}
        Data sources — MembershipStakeVault.attributedStake {short(book.addresses.MembershipStakeVault)} · CitrateMemberSBT.balanceOf {short(book.addresses.CitrateMemberSBT)} · /userinfo entitlement
      </div>
    </div>
  );
}

function SettleRow({ k, v, accent }: { k: string; v: string; accent?: boolean }) {
  return (
    <div style={{ display: "flex", justifyContent: "space-between", gap: 12 }}>
      <span style={{ fontSize: 13, color: "var(--tx-2)" }}>{k}</span>
      <span className="mono tabular" style={{ fontSize: 13, color: accent ? "var(--accent-text)" : undefined }}>
        {v}
      </span>
    </div>
  );
}

function S6({ store, s }: { store: Store; s: AppState }) {
  // The REAL height. This used to subtract `(100 - syncPct) * 14` to fake a "catching up" number
  // off the 0/100 stub, a fabricated height (Rule 1). Show what the node actually has; progress is
  // the measured sync line (syncProgress.ts: blocks behind the public head, rate and ETA only once
  // measured), never a percentage (contract §1).
  // C1-V V10: 0 is "no reading yet" (eth_blockNumber returns 0x0 on error), never a height.
  const heightStr = s.height > 0 ? fmtI(s.height) : "—";
  return (
    <div className="cc-fade-up" style={{ display: "flex", flexDirection: "column", gap: 20 }}>
      <div style={eyebrow}>S6 · Node ignition</div>
      <div style={h1}>Run your node</div>
      <p style={body}>
        A full node follows the chain, checks and executes every block, and serves your reads locally. It produces blocks only once it is an active validator. It
        runs as a supervised process with encryption at rest on by default, and it survives this window closing.
      </p>
      <div className="surface" style={{ padding: 20, display: "flex", flexDirection: "column", gap: 16 }}>
        {s.node === "off" && (
          <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", gap: 14 }}>
            <div>
              <div style={{ fontSize: 15, fontWeight: 500 }}>citrate-node · full node</div>
              <div className="mono" style={{ fontSize: 11, color: "var(--tx-3)" }}>
                testnet · data encrypted at rest · key in OS keyring
              </div>
            </div>
            <button className="btn btn-primary btn-lg" onClick={() => store.startNode()}>
              Run my node
            </button>
          </div>
        )}
        {s.node === "prov" && (
          <div style={{ display: "flex", alignItems: "center", gap: 16 }}>
            <div style={{ width: 44, height: 44, flexShrink: 0 }}>
              <LoaderMark size={44} />
            </div>
            <div>
              <div style={{ fontSize: 14, fontWeight: 500 }}>Provisioning</div>
              <div className="mono" style={{ fontSize: 11, color: "var(--tx-3)" }}>
                data dir created · master key sealed to keyring · bootnodes loaded
              </div>
            </div>
          </div>
        )}
        {/* The node supervisor failed to start (e.g. a stale RocksDB LOCK held by an
            orphaned node from a prior run, no disk, or a missing sidecar). Before this
            branch existed, `error` rendered NOTHING — an empty panel with no way to
            recover, so a failed start looked like a hang. Surface it honestly and let
            the member retry (`startNode` fails closed again if it still can't start). */}
        {s.node === "error" && (
          <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", gap: 14 }}>
            <div>
              <div style={{ fontSize: 15, fontWeight: 500 }}>Node didn’t start</div>
              <div className="mono" style={{ fontSize: 11, color: "var(--tx-3)" }}>
                the supervisor could not launch the node — retry, or check the log for the reason
              </div>
            </div>
            <button className="btn btn-primary btn-lg" onClick={() => store.startNode()}>
              Retry
            </button>
          </div>
        )}
        {s.node === "syncing" && (
          <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
            <div style={{ fontSize: 14, fontWeight: 500 }}>Syncing chain 40204</div>
            <div className="mono tabular" title={SYNC_SOURCE} style={{ fontSize: 13, color: "var(--accent-text)" }}>
              {syncLine(s.syncProgress)}
            </div>
            <div style={{ display: "flex", gap: 20 }}>
              <span className="mono tabular" style={{ fontSize: 12, color: "var(--tx-2)" }}>
                height {heightStr}
              </span>
              <span className="mono tabular" style={{ fontSize: 12, color: "var(--tx-2)" }}>
                peers {s.peers}
              </span>
            </div>
            {s.peers > 0 && (
              <div style={{ fontSize: 12, color: "var(--tx-3)" }}>
                You’re connected — no need to wait. Continue below and your node keeps syncing in the
                background; chain balances catch up on the Dashboard as it does.
              </div>
            )}
          </div>
        )}
        {/* C1-V (contract v1.1 §1): `validating` is set only when the key is isActive AND its
            validatorInfo.actEpoch <= currentEpoch, never from a stake amount, so this card uses the
            §1 word and the consensus-lag note. */}
        {s.node === "validating" && (
          <div style={{ display: "flex", alignItems: "center", gap: 14 }}>
            <span style={{ width: 10, height: 10, borderRadius: 999, background: "var(--accent)", animation: "ccPulse 2.4s var(--ease-standard) infinite", flexShrink: 0 }}></span>
            <div style={{ flex: 1 }}>
              <div style={{ fontSize: 15, fontWeight: 500 }}>Validator active</div>
              <div className="mono" style={{ fontSize: 11, color: "var(--tx-3)" }}>
                synced · activation epoch reached · stake {fmtStake(readStakeSalt(s))} · {CONSENSUS_LAG_NOTE}
              </div>
            </div>
          </div>
        )}
        {/* SYNCED = the node is caught up. Activating the validator bond is its own step now
            (S7 · Activate validator, Join-8), which explains the transaction and opens the
            Signature Ceremony only on the member's click. */}
        {/* Registered stake that is not active yet (pending activation, exiting, or not read):
            the bond is already staked, so no "Stake my bond" here. */}
        {s.node === "synced" && s.bondedStake > 0 && (
          <div style={{ display: "flex", alignItems: "center", gap: 14 }}>
            <span style={{ width: 10, height: 10, borderRadius: 999, background: "var(--accent)", flexShrink: 0 }}></span>
            <div style={{ flex: 1 }}>
              <div style={{ fontSize: 15, fontWeight: 500 }}>Synced · {validatorWord(s.bondedStake, s.validatorActivation, s.hasGrant) ?? "— · registry not read yet"}</div>
              <div className="mono" style={{ fontSize: 11, color: "var(--tx-3)" }}>
                stake {fmtStake(readStakeSalt(s))} in the ValidatorRegistry
              </div>
            </div>
          </div>
        )}
        {/* C1-V2: after a full exit (or a slash) the registry stake reads 0, but validatorInfo.status
            says Exiting / Slashed: show that word, and no "Stake my bond". */}
        {/* #348/#349 review: an already-activated bond (MemberBond.activated() = true) with no
            registered stake has exited or been slashed; it can't activate again, so it gets the
            status card (word or "—"), never "Stake my bond". */}
        {s.node === "synced" && !(s.bondedStake > 0) && !canOfferActivation(s.bondedStake, s.validatorActivation, s.bondActivated) && (s.hasGrant || s.validatorActivation?.status === 2 || s.validatorActivation?.status === 3) && (
          <div style={{ display: "flex", alignItems: "center", gap: 14 }}>
            <span style={{ width: 10, height: 10, borderRadius: 999, background: "var(--warn, var(--accent))", flexShrink: 0 }}></span>
            <div style={{ flex: 1 }}>
              <div style={{ fontSize: 15, fontWeight: 500 }}>Synced · {validatorDash(s.bondedStake, s.validatorActivation, s.hasGrant, s.bondActivated)}</div>
              <div className="mono" style={{ fontSize: 11, color: "var(--tx-3)" }}>
                ValidatorRegistry.validatorInfo status for this node&rsquo;s key; no registered stake
              </div>
            </div>
          </div>
        )}
        {/* #348 review: activation is offered only for a bond that reads not activated (activated() =
            false). Join-8 (#354): activating is its own step (S7 · Activate validator), which explains
            the transaction and opens the Signature Ceremony only on the member's click, so this card
            points there and has no button. */}
        {s.node === "synced" && !(s.bondedStake > 0) && (canOfferActivation(s.bondedStake, s.validatorActivation, s.bondActivated) || (!s.hasGrant && s.validatorActivation?.status !== 2 && s.validatorActivation?.status !== 3)) && (
          <div style={{ display: "flex", alignItems: "center", gap: 14 }}>
            <span style={{ width: 10, height: 10, borderRadius: 999, background: "var(--warn, var(--accent))", flexShrink: 0 }}></span>
            <div style={{ flex: 1 }}>
              <div style={{ fontSize: 15, fontWeight: 500 }}>Synced · {validatorDash(s.bondedStake, s.validatorActivation, s.hasGrant, s.bondActivated)}</div>
              <div className="mono" style={{ fontSize: 11, color: "var(--tx-3)" }}>
                {s.hasGrant ? "next: activate your validator, after the local model below" : "caught up with the network"}
              </div>
            </div>
          </div>
        )}
      </div>
      <div className="mono" style={dataSrc}>
        Data sources — local node RPC · node-agent supervision API 127.0.0.1:19600
      </div>

      {/* S6.5 — local model in-flow (BC-3). The local model runs independently of chain sync, so we
          do NOT make the member wait for a FULL sync to proceed: once the node is confirmed syncing
          (up + at least one peer, so it is genuinely pulling the chain), the model step + "Enter your
          dashboard" become available. The node keeps syncing IN THE BACKGROUND inside the app, and
          the Node/Dashboard surfaces show its live progress. */}
      {(s.node === "syncing" ? s.peers > 0 : s.node === "validating" || s.node === "synced") && (
        <ModelStep store={store} s={s} />
      )}

      {/* HUP-S7.4 (US-7.1) — give Hermes an on-chain identity (AgentSBT), approved in the
          Signature Ceremony. Optional and never blocking: "Enter your dashboard" stays above. */}
      {(s.node === "syncing" ? s.peers > 0 : s.node === "validating" || s.node === "synced") && <AgentIdentityStep store={store} />}
      {(s.node === "syncing" ? s.peers > 0 : s.node === "validating" || s.node === "synced") && (
        <PersonaStep store={store} s={s} />
      )}
    </div>
  );
}

// HUP-S3.3 + S3.7 — optional: pick Hermes's voice. Skipping keeps the default voice (nothing
// changes); the same picker lives in Settings, under App.
export function PersonaStep({ store, s }: { store: Store; s: AppState }) {
  return (
    <div className="surface" data-testid="onboarding-persona-step" style={{ padding: 18, display: "flex", flexDirection: "column", gap: 8, marginTop: 14 }}>
      <span className="eyebrow">Optional · Hermes's voice</span>
      <PersonaPicker
        compact
        api={bridge.agentHarness}
        chosen={s.hermesPersona}
        custom={s.customPersonas}
        onChoose={(p) => store.chooseHermesPersona(p)}
        onAddCustom={(p) => store.addCustomPersona(p)}
        onRemoveCustom={(id) => store.removeCustomPersona(id)}
      />
    </div>
  );
}

// BC-3.3 — the S6.5 local-model step. An honest download + verify of the Gemma
// GGUF (4.96 GB) with a REAL progress bar driven by `model.status()`, a verify,
// and a SKIP that honestly routes chat to the gateway/demo. "Enter your dashboard"
// is gated behind the model being READY (a real verify) OR an explicit SKIP — the
// captions name the real source + the pinned SHA-256, and no "verified" is ever
// fabricated (Rule 1). The `ready` state only appears from a real verify.
export function ModelStep({ store, s }: { store: Store; s: AppState }) {
  const pct =
    s.modelTotalBytes > 0 ? Math.min(100, Math.round((s.modelDownloadedBytes / s.modelTotalBytes) * 1000) / 10) : 0;
  const gb = (bytes: number) => (bytes / 1e9).toFixed(2);
  // #30 (Luke 2026-09-11) — don't make the member watch the 5 GB meter. Auto-START the
  // Gemma download in the background as soon as the model step is reached (no "Download"
  // click), and let them ENTER the dashboard immediately — chat runs on the gateway until
  // the local model verifies, and the dashboard shows the download + node-sync progress.
  // The download runs off the main thread (v0.2.3 fix), so it never blocks the UI.
  const autoDlRef = useRef(false);
  useEffect(() => {
    if (autoDlRef.current) return;
    autoDlRef.current = true;
    if (s.modelState === "notPresent" && !s.modelSkipped) store.startModelDownload();
  }, [store, s.modelState, s.modelSkipped]);
  // Never gate entry on the download finishing (the old trap: a stalled multi-GB pull
  // left "Enter your dashboard" greyed out). It is always available; the model keeps
  // downloading in the background.
  const canEnter = true;
  const sha = "90ce9812…e0313e9f"; // the pinned SHA-256 (abbreviated for display)

  return (
    <div className="surface" style={{ padding: 20, display: "flex", flexDirection: "column", gap: 16, marginTop: 4 }} data-testid="model-step">
      <div style={eyebrow}>S6.5 · Local model</div>
      <div style={{ display: "flex", alignItems: "baseline", justifyContent: "space-between", gap: 12 }}>
        <div style={{ fontSize: 15, fontWeight: 500 }}>Download the local model (4.96 GB)</div>
        <span className="mono" style={{ fontSize: 10, letterSpacing: ".1em", textTransform: "uppercase", color: "var(--tx-3)" }}>
          gemma · Q4_K_M · GGUF
        </span>
      </div>
      <p style={body}>
        Run chat on-device with a bundled llama-server, no data leaves this machine. The download is streamed and resumable, and
        verified against a pinned SHA-256 before it is ever used. While it downloads — or if you skip — chat runs on the gateway.
      </p>

      {/* HUP-S1.6 (US-1.6) — the local hardware probe's tier + rationale, with a persisted override. */}
      <TierPanel />

      {(s.modelState === "notPresent" || s.modelState === "error") && !s.modelSkipped && (
        <div style={{ display: "flex", gap: 10 }}>
          <button className="btn btn-primary btn-lg" onClick={() => store.startModelDownload()}>
            {s.modelState === "error" ? "Retry download" : "Download local model"}
          </button>
          <button className="btn btn-ghost btn-lg" onClick={() => store.skipModel()}>
            Skip — use the gateway
          </button>
        </div>
      )}

      {s.modelState === "error" && s.modelError && (
        <div className="mono" style={{ fontSize: 11.5, color: "var(--danger)" }} data-testid="model-error">
          {s.modelError}
        </div>
      )}

      {s.modelState === "downloading" && (
        <div style={{ display: "flex", flexDirection: "column", gap: 12 }} data-testid="model-downloading">
          <div style={{ display: "flex", justifyContent: "space-between", alignItems: "baseline" }}>
            <div style={{ fontSize: 14, fontWeight: 500 }}>Downloading</div>
            <div className="mono tabular" style={{ fontSize: 13, color: "var(--accent-text)" }} data-testid="model-pct">
              {pct}%
            </div>
          </div>
          <div style={{ height: 6, background: "var(--srf-inset)", borderRadius: 999, overflow: "hidden", border: "1px solid var(--line-1)" }}>
            <div style={{ height: "100%", background: "var(--accent)", width: pct + "%", transition: "width .5s var(--ease-standard)" }}></div>
          </div>
          <span className="mono tabular" style={{ fontSize: 12, color: "var(--tx-2)" }} data-testid="model-bytes">
            {gb(s.modelDownloadedBytes)} / {gb(s.modelTotalBytes)} GB
          </span>
          {/* A download can stall (dropped connection) WITHOUT the supervisor
              marking it "error" — leaving this state with no Skip, no Retry, and a
              greyed "Enter your dashboard" (a trap observed 2026-08-05). Always offer
              an escape while downloading: Skip proceeds to the gateway now; Restart
              re-triggers the streamed (resumable) fetch. The model is optional —
              chat runs on the gateway either way. */}
          <div style={{ display: "flex", gap: 10, marginTop: 4 }}>
            <button className="btn btn-ghost" onClick={() => store.skipModel()}>
              Skip — use the gateway
            </button>
            <button className="btn btn-ghost" onClick={() => store.startModelDownload()}>
              Restart download
            </button>
          </div>
        </div>
      )}

      {s.modelState === "verifying" && (
        <div style={{ display: "flex", alignItems: "center", gap: 16 }} data-testid="model-verifying">
          <div style={{ width: 40, height: 40, flexShrink: 0 }}>
            <LoaderMark size={40} />
          </div>
          <div>
            <div style={{ fontSize: 14, fontWeight: 500 }}>Verifying SHA-256</div>
            <div className="mono" style={{ fontSize: 11, color: "var(--tx-3)" }}>
              streaming the file through SHA-256 · comparing to the pinned hash
            </div>
          </div>
        </div>
      )}

      {s.modelState === "ready" && (
        <div style={{ display: "flex", alignItems: "center", gap: 12 }} data-testid="model-ready">
          <span className="cc-stamp" style={{ width: 26, height: 26, borderRadius: 999, background: "var(--ok-bg)", border: "1px solid var(--ok)", color: "var(--ok)", display: "inline-flex", alignItems: "center", justifyContent: "center" }}>
            {checkBig(14)}
          </span>
          <div style={{ fontSize: 14, fontWeight: 500 }}>Local model verified — chat runs on-device</div>
        </div>
      )}

      {s.modelSkipped && s.modelState !== "ready" && (
        <div className="mono" style={{ fontSize: 11.5, color: "var(--tx-3)" }} data-testid="model-skipped">
          Skipped — chat runs on the gateway. You can download the local model later in Settings.
        </div>
      )}

      <div style={{ borderTop: "1px solid var(--line-1)", paddingTop: 12, display: "flex", alignItems: "center", justifyContent: "space-between", gap: 12 }}>
        <div className="mono" style={{ fontSize: 10, letterSpacing: ".08em", color: "var(--tx-3)" }}>
          huggingface.co/ggml-org/gemma-4-E4B-it-GGUF · sha256 {sha}
        </div>
        <button className="btn btn-primary" disabled={!canEnter} onClick={() => store.onS6Continue()} data-testid="model-enter">
          {s.hasGrant && s.stage === "s6" ? "Continue to Activate validator" : "Enter your dashboard"}
        </button>
      </div>
    </div>
  );
}

/** S0's network line: "Citrate betanet" once the genesis check passes (UI truth contract §2, §6). */
function NetworkName({ style }: { style: CSSProperties }) {
  return <div style={style}>{useNetworkLabel()}</div>;
}

// =====================================================================
// Join-8 · S7 Activate validator. Runs after node sync for a member with a granted bond. Every
// state word, condition and duration follows the UI truth contract v1 (§1, §2, §5); the logic
// lives in ./activateValidator.ts. Activation only OPENS the Signature Ceremony (Rule 3).
// =====================================================================
const ACTIVATION_POLL_MS = 15_000;

function GateRow({ ok, label, detail }: { ok: boolean | null; label: string; detail: string }) {
  const color = ok === true ? "var(--ok)" : ok === false ? "var(--danger)" : "var(--tx-3)";
  return (
    <div style={{ display: "flex", alignItems: "baseline", gap: 10 }}>
      <span className="mono" style={{ width: 14, flexShrink: 0, color }}>{ok === true ? "✓" : ok === false ? "×" : "—"}</span>
      <span className="mono" style={{ fontSize: 10, letterSpacing: ".1em", textTransform: "uppercase", color: "var(--tx-3)", width: 120, flexShrink: 0 }}>
        {label}
      </span>
      <span style={{ fontSize: 13, color: ok === false ? "var(--tx-1)" : "var(--tx-2)" }}>{detail}</span>
    </div>
  );
}

export function S7({ store, s }: { store: Store; s: AppState }) {
  useEffect(() => {
    void store.refreshActivation();
    const t = setInterval(() => void store.refreshActivation("poll"), ACTIVATION_POLL_MS);
    return () => clearInterval(t);
  }, [store]);
  const a = s.activation;
  const linked = store.walletIsLinked();
  const gate = activationGate(a, linked);
  const phase = activationPhase(a, linked, s.s7TxHash);
  const vs = validatorState(a);
  const settled = phase === "registered" || phase === "active" || phase === "exiting" || phase === "exited" || phase === "exitSettled" || phase === "slashed";
  const mismatch = keyMismatch(a);
  return (
    <div className="cc-fade-up" style={{ display: "flex", flexDirection: "column", gap: 20 }} data-testid="s7">
      <div style={eyebrow}>S7 · Activate validator</div>
      <div style={h1}>Activate your validator</div>
      <p style={body}>{ACTIVATE_EXPLANATION}</p>

      <div className="surface" style={{ padding: "18px 20px", display: "flex", flexDirection: "column", gap: 10 }}>
        <div style={{ display: "flex", alignItems: "baseline", justifyContent: "space-between", gap: 12 }}>
          <span style={eyebrow}>Validator status</span>
          <span style={{ fontSize: 15, fontWeight: 500 }} data-testid="s7-state">{vs.word ?? "—"}</span>
        </div>
        <p style={{ fontSize: 12.5, lineHeight: 1.55, color: "var(--tx-2)", margin: 0 }}>{vs.detail}</p>
        {mismatch && (
          <p style={{ fontSize: 12.5, lineHeight: 1.55, color: "var(--warn, var(--danger))", margin: 0 }} data-testid="s7-key-mismatch">
            {mismatch}
          </p>
        )}
        <p style={{ fontSize: 12, lineHeight: 1.55, color: "var(--tx-3)", margin: 0, borderTop: "1px solid var(--line-1)", paddingTop: 10 }}>
          {activationGrantCopy(a)}
        </p>
      </div>

      {!settled && (
        <div className="surface" style={{ padding: "18px 20px", display: "flex", flexDirection: "column", gap: 8 }} data-testid="s7-gate">
          <div style={eyebrow}>Before activation</div>
          {gate.checks.map((c) => (
            <GateRow key={c.key} ok={c.ok} label={c.label} detail={c.detail} />
          ))}
          {a && !a.nodeKeyReady && a.bondActivated === false && (
            <p style={{ fontSize: 12, lineHeight: 1.55, color: "var(--tx-3)", margin: "6px 0 0" }} data-testid="s7-key">
              Activation stays hidden until this node has its validator key, because the registration is signed with it. The node mints the key
              when block production is allowed for it{s.miningHold ? ". Right now: " + s.miningHold : "."} This step checks again every 15 s.
            </p>
          )}
        </div>
      )}

      {phase === "ready" && (
        <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
          <div>
            <button className="btn btn-primary btn-lg" onClick={() => void store.requestValidatorActivation()} data-testid="s7-activate">
              Activate validator
            </button>
          </div>
          <p className="mono" style={{ fontSize: 11, color: "var(--tx-3)", margin: 0 }}>
            Opens the Signature Ceremony for one MemberBond.activate transaction. It moves no SALT from your wallet; your wallet pays the gas.
          </p>
        </div>
      )}

      <ActivationTxPanel store={store} s={s} testId="s7" />

      {s.activationError && (
        <p className="mono" style={{ fontSize: 11, color: "var(--warn, var(--tx-3))", margin: 0 }} data-testid="s7-read-error">
          Last read failed: {s.activationError}. {a ? "Showing the previous read." : ""}
        </p>
      )}

      {settled ? (
        <div>
          <button className="btn btn-primary btn-lg" onClick={() => store.onEnter()} data-testid="s7-enter">
            Enter your dashboard
          </button>
        </div>
      ) : (
        <div style={{ borderTop: "1px solid var(--line-2)", paddingTop: 16, display: "flex", flexDirection: "column", gap: 8 }}>
          <div>
            <button className="btn btn-ghost btn-lg" onClick={() => store.onS7Skip()} data-testid="s7-skip">
              Skip for now
            </button>
          </div>
          <p className="mono" style={{ fontSize: 11, color: "var(--tx-3)", margin: 0 }}>
            Enter the app without activating. Your bond stays locked, not staked, and you can activate later from Node.
          </p>
        </div>
      )}

      <div className="mono" style={dataSrc}>
        Data sources — MembershipStakeVault.bondOf · MemberBond.activated / principal / unlockBlock / unlockTimestamp · CitrateMemberSBT.isActive ·
        ValidatorRegistry.validatorInfo / isActive / currentEpoch / EPOCH · ValidatorRegistry exit logs (UnbondInitiated / Evicted / Slashed /
        Withdrawn) · local eth_syncing · block time{" "}
        {a?.blockTimeMs != null ? fmtDuration(a.blockTimeMs) : "—"} (measured over the last 100 blocks)
      </div>
    </div>
  );
}
