import marqueeBlack from "../assets/brand/citrate_marquee_black.svg";
import { LoaderMark } from "../components/LoaderMark";
import { Store } from "../shell/store";
import { AppState, fmtSaltFromWei } from "../shell/state";

const fmtI = (n: number) => Math.round(n).toLocaleString("en-US");
const short = (h: string) => (h ? h.slice(0, 6) + "…" + h.slice(-4) : "—");

const eyebrow: React.CSSProperties = {
  fontFamily: "var(--font-mono)",
  fontSize: 11,
  fontWeight: 500,
  letterSpacing: ".14em",
  textTransform: "uppercase",
  color: "var(--tx-3)",
};
const h1: React.CSSProperties = {
  fontFamily: "var(--font-display)",
  fontWeight: 420,
  fontSize: 32,
  lineHeight: 1.12,
  letterSpacing: "-0.011em",
};
const body: React.CSSProperties = { fontSize: 15, lineHeight: 1.6, color: "var(--tx-2)", margin: 0 };
const dataSrc: React.CSSProperties = {
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
  const stageOrder = ["s1", "s2", "s3", "s4", "s5", "s6"];
  const cur = stageOrder.indexOf(s.stage);
  const stages: [string, string][] = [
    ["s1", "Sign in"],
    ["s2", "Verify identity"],
    ["s3", "Membership"],
    ["s4", "Wallet ready"],
    ["s5", "Grant + stake"],
    ["s6", "Node ignition"],
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
              One membership funds your wallet, covers your validator stake, and opens every Citrate application — on your hardware, with keys that never leave this
              machine.
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
          <div style={eyebrow}>Chain 40204 · testnet-beta</div>
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
          <div style={{ ...eyebrow, marginBottom: 6 }}>Membership onboarding</div>
          {stages.map(([, label], i) => {
            const done = i < cur;
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
                  {done ? "✓" : String(i + 1)}
                </span>
                <span style={{ flex: 1, fontSize: 13, fontWeight: active ? 500 : 400, color: active ? "var(--tx-1)" : done ? "var(--tx-2)" : "var(--tx-3)" }}>{label}</span>
                <span className="mono" style={{ fontSize: 9, letterSpacing: ".12em", textTransform: "uppercase", color: "var(--tx-3)" }}>
                  {done ? "done" : active ? "now" : ""}
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
            {s.stage === "s1" && <S1 store={store} s={s} />}
            {s.stage === "s2" && <S2 store={store} s={s} />}
            {s.stage === "s3" && <S3 store={store} s={s} />}
            {s.stage === "s4" && <S4 store={store} s={s} />}
            {s.stage === "s5" && <S5 store={store} s={s} />}
            {s.stage === "s6" && <S6 store={store} s={s} />}
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
        This app never shows credential fields. Your system browser opens auth.citrate.ai — passkey, email, Google, or sign-in-with-Ethereum. That is the anti-phishing
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
              auth.citrate.ai · loopback PKCE · 127.0.0.1:49821
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

function S2({ store, s }: { store: Store; s: AppState }) {
  return (
    <div className="cc-fade-up" style={{ display: "flex", flexDirection: "column", gap: 20 }}>
      <div style={eyebrow}>S2 · Verify</div>
      <div style={h1}>Verify your identity</div>
      <p style={body}>
        Verification is optional — you can finish it any time. Your membership only needs your payment; identity verification unlocks certain features and upgrades you
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
              Continue to membership
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
            Skip for now — continue to membership
          </button>
          <p className="mono" style={{ fontSize: 11, color: "var(--tx-3)", margin: 0 }}>
            Membership needs only your payment. You keep the <span className="mono">commercial</span> tier now and can finish KYC any time to upgrade to
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

export function S3({ store, s }: { store: Store; s: AppState }) {
  const items = [
    "Tier features across the app — chat on the gateway, docs at member tier, gated downloads",
    "32,000 SALT staked to your validator — the grant covers your stake for validation work",
    "Membership SBT — the non-transferable on-chain record of your seat",
    "Commissary access — apps, SDKs, docs, and services at your tier",
  ];
  return (
    <div className="cc-fade-up" style={{ display: "flex", flexDirection: "column", gap: 20 }}>
      <div style={eyebrow}>S3 · Membership</div>
      <div style={h1}>Yearly membership</div>
      <div style={{ display: "grid", gridTemplateColumns: "1fr 1.2fr 1fr", gap: 12 }}>
        <div style={{ border: "1px solid var(--line-1)", background: "var(--srf-1)", borderRadius: "var(--r-2)", padding: 16, display: "flex", flexDirection: "column", gap: 6 }}>
          <div style={eyebrow}>Free</div>
          <div style={{ fontFamily: "var(--font-display)", fontSize: 24, fontWeight: 420 }}>$0</div>
          <p style={{ fontSize: 12, lineHeight: 1.5, color: "var(--tx-2)", margin: 0 }}>Wallet, agent harness, chain reads, marketplace view. No staking, no grant.</p>
        </div>
        <div style={{ border: "2px solid var(--tx-1)", background: "var(--srf-2)", borderRadius: "var(--r-2)", padding: 16, display: "flex", flexDirection: "column", gap: 6, position: "relative" }}>
          <div style={{ ...eyebrow, color: "var(--accent-text)" }}>Pilot · selected</div>
          <div style={{ fontFamily: "var(--font-display)", fontSize: 24, fontWeight: 420 }}>
            $48<span style={{ fontSize: 13, color: "var(--tx-3)" }}> / year</span>
          </div>
          <p style={{ fontSize: 12, lineHeight: 1.5, color: "var(--tx-2)", margin: 0 }}>Full membership. Everything listed below.</p>
        </div>
        <div style={{ border: "1px solid var(--line-1)", background: "var(--srf-1)", borderRadius: "var(--r-2)", padding: 16, display: "flex", flexDirection: "column", gap: 6 }}>
          <div style={eyebrow}>Enterprise</div>
          <div style={{ fontFamily: "var(--font-display)", fontSize: 24, fontWeight: 420 }}>Custom</div>
          <p style={{ fontSize: 12, lineHeight: 1.5, color: "var(--tx-2)", margin: 0 }}>Org seats, on-prem topology, custom compliance plane. Contact us.</p>
        </div>
      </div>
      <div className="surface" style={{ padding: "18px 20px", display: "flex", flexDirection: "column", gap: 10 }}>
        <div style={eyebrow}>What membership carries</div>
        {items.map((text, i) => (
          <div key={i} style={{ display: "flex", gap: 10, alignItems: "baseline" }}>
            <span style={{ width: 5, height: 5, borderRadius: 999, background: "var(--accent)", flexShrink: 0, position: "relative", top: -2 }}></span>
            <span style={{ fontSize: 13.5, lineHeight: 1.5, color: "var(--tx-1)" }}>{text}</span>
          </div>
        ))}
        <p style={{ fontSize: 11.5, lineHeight: 1.5, color: "var(--tx-3)", margin: "6px 0 0" }}>
          The grant is a membership benefit that arrives staked — it covers your stake for work as a validator. It is not purchased, and it is not an investment.
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
            <button className="btn btn-primary btn-lg" onClick={() => store.onS3Pay()}>
              Check out in your browser · $48
            </button>
          )}
        </div>
      )}
      {s.s3 === "paying" && (
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
  // Q-A.4b item 10 — for a SIGNED-IN user the wallet address must come from a REAL
  // `wallet_address` claim (walletFromClaim). Without it, `s.walletAddr` is the
  // deterministic persona `makeAddr` seed — a fabricated wallet for a real account
  // — so we show "—"/pending instead. In the web-dev/persona path (signedIn=false)
  // the labelled persona address is the honest preview and renders as before.
  const walletKnown = !s.signedIn || s.walletFromClaim;
  const walletDisplay = walletKnown ? s.walletAddr : "— · pending wallet_address claim";
  return (
    <div className="cc-fade-up" style={{ display: "flex", flexDirection: "column", gap: 20 }}>
      <div style={eyebrow}>S4 · Wallet ready</div>
      <div style={h1}>Your smart wallet already exists</div>
      <p style={body}>
        The address is deterministic from your identity — predicted before anything is deployed. The wallet itself deploys lazily with your first transaction, gas
        sponsored.
      </p>
      <div className="surface" style={{ padding: "18px 20px", display: "flex", flexDirection: "column", gap: 12 }}>
        <div className="lbl">Smart wallet · ERC-4337</div>
        <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
          <span className="mono" style={{ fontSize: 14, wordBreak: "break-all" }}>{walletDisplay}</span>
          {walletKnown && (
            <button className="btn btn-ghost btn-sm" onClick={() => store.copy(s.walletAddr, "Address copied")}>
              Copy
            </button>
          )}
        </div>
        <div style={{ borderTop: "1px solid var(--line-1)", paddingTop: 12, display: "flex", flexDirection: "column", gap: 8 }}>
          <Row k="Signer" v={<span style={{ fontSize: 13 }}>Passkey validator (WebAuthn P-256) — enrolled with your identity</span>} />
          <Row k="Recovery" v={<span style={{ fontSize: 13 }}>Guardian module — configure guardians any time in Settings</span>} />
          <Row k="Local key" v={<span style={{ fontSize: 13 }}>Optional signing key in this machine's keystore — create later in Keys &amp; security</span>} />
        </div>
      </div>
      <div>
        <button className="btn btn-primary btn-lg" onClick={() => { store.setState({ stage: "s5" }); store.save(); }}>
          Continue to the grant ceremony
        </button>
      </div>
      <div className="mono" style={dataSrc}>
        Data source — wallet_address claim · /aa/address · EntryPoint 0x077F…54Ef
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
      <div style={eyebrow}>S5 · Grant + stake ceremony</div>
      {s.s5 === "idle" && (
        <div style={{ display: "flex", flexDirection: "column", gap: 20 }}>
          <div style={h1}>32,000 SALT — staked for you</div>
          <p style={body}>
            Your membership grant arrives already staked: the treasury deposits it into the MembershipStakeVault, attributed to your wallet, and the vault stakes it. It
            covers your validator stake for the membership year. The principal stays vaulted until mainnet release; the rewards your node earns are yours. You approve a
            single sponsored transaction.
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
          <div style={eyebrow}>Arriving · staking</div>
          <div className="tabular" style={{ fontFamily: "var(--font-display)", fontWeight: 380, fontSize: 72, lineHeight: 1, letterSpacing: "-0.022em", color: "var(--tx-1)" }}>
            {fmtI(s.s5n)}
          </div>
          <div style={{ ...eyebrow, color: "var(--accent-text)" }}>SALT → MembershipStakeVault → LiquidStakingPool</div>
          <div className="mono" style={{ fontSize: 11, color: "var(--tx-3)" }}>
            treasury UserOp · gas sponsored · first-op category
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
              <div style={{ ...eyebrow, color: "var(--accent-text)" }}>SALT staked · settled</div>
            </div>
            <div style={{ display: "flex", flexDirection: "column", gap: 8, borderTop: "1px solid var(--line-1)", paddingTop: 12 }}>
              {/* F1 (Rule 1): every row traces to a real read. Staked position is the
                  REAL attributedStake (wei→SALT); the SBT row shows only "minted"
                  (confirmed by balanceOf==1 — the token id was never read); the
                  honest on-chain anchor is the member/vault address, NOT a fabricated
                  tx hash (this app does not broadcast the grant tx). */}
              <SettleRow k="Staked position" v={`${fmtSaltFromWei(s.s5StakeWei)} SALT · vaulted principal`} />
              <SettleRow k="Membership SBT" v="CitrateMemberSBT · minted" />
              <SettleRow k="Member wallet" v={short(s.walletAddr)} accent />
            </div>
            <p style={{ fontSize: 12, lineHeight: 1.55, color: "var(--tx-3)", margin: 0 }}>
              Principal locked for the membership term and until mainnet release policy unlocks vaulted grants. Validator rewards from your node's work accrue to you.
              Self-added stake carries a 7-day withdrawal lockup.
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
        Data sources — MembershipStakeVault.attributedStake 0x0ace…267e · CitrateMemberSBT.balanceOf 0x7be0…a7c4 · /userinfo entitlement
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
  const syncPct = s.syncPct | 0;
  const heightStr = fmtI(s.height - Math.round((100 - s.syncPct) * 14));
  return (
    <div className="cc-fade-up" style={{ display: "flex", flexDirection: "column", gap: 20 }}>
      <div style={eyebrow}>S6 · Node ignition</div>
      <div style={h1}>Run your node</div>
      <p style={body}>
        A full node — it validates, executes, and builds blocks. It runs as a supervised process with encryption at rest on by default, and it survives this window
        closing.
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
        {s.node === "syncing" && (
          <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
            <div style={{ display: "flex", justifyContent: "space-between", alignItems: "baseline" }}>
              <div style={{ fontSize: 14, fontWeight: 500 }}>Syncing chain 40204</div>
              <div className="mono tabular" style={{ fontSize: 13, color: "var(--accent-text)" }}>
                {syncPct}%
              </div>
            </div>
            <div style={{ height: 6, background: "var(--srf-inset)", borderRadius: 999, overflow: "hidden", border: "1px solid var(--line-1)" }}>
              <div style={{ height: "100%", background: "var(--accent)", width: syncPct + "%", transition: "width .5s var(--ease-standard)" }}></div>
            </div>
            <div style={{ display: "flex", gap: 20 }}>
              <span className="mono tabular" style={{ fontSize: 12, color: "var(--tx-2)" }}>
                height {heightStr} / {fmtI(s.height)}
              </span>
              <span className="mono tabular" style={{ fontSize: 12, color: "var(--tx-2)" }}>
                peers {s.peers}
              </span>
            </div>
          </div>
        )}
        {(s.node === "validating" || s.node === "synced") && (
          <div style={{ display: "flex", alignItems: "center", gap: 14 }}>
            <span style={{ width: 10, height: 10, borderRadius: 999, background: "var(--accent)", animation: "ccPulse 2.4s var(--ease-standard) infinite", flexShrink: 0 }}></span>
            <div style={{ flex: 1 }}>
              <div style={{ fontSize: 15, fontWeight: 500 }}>Validating</div>
              <div className="mono" style={{ fontSize: 11, color: "var(--tx-3)" }}>
                synced · stake 32,000 SALT ≥ minimum · eligible for proposer election
              </div>
            </div>
          </div>
        )}
      </div>
      <div className="mono" style={dataSrc}>
        Data sources — local node RPC · node-agent supervision API 127.0.0.1:19600
      </div>

      {/* S6.5 — local model in-flow (BC-3). Appears once the node is up. */}
      {(s.node === "validating" || s.node === "synced") && <ModelStep store={store} s={s} />}
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
  const canEnter = s.modelState === "ready" || s.modelSkipped;
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
        <button className="btn btn-primary" disabled={!canEnter} onClick={() => store.onEnter()} data-testid="model-enter">
          Enter your dashboard
        </button>
      </div>
    </div>
  );
}
