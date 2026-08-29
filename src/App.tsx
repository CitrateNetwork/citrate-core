import { useEffect, useState } from "react";
import { WagmiProvider } from "wagmi";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { wagmiConfig } from "./wagmi";
import { store, useStore } from "./shell/store";
import { AppState } from "./shell/state";
import { bridge } from "./bridge";
import { Onboarding } from "./onboarding/Onboarding";
import { LoaderMark } from "./components/LoaderMark";
import marqueeBlack from "./assets/brand/citrate_marquee_black.svg";
import { Sidebar } from "./shell/Sidebar";
import { SignatureCeremony, WalletReviewModal, Coach, Toast, DemoPanel } from "./shell/Chrome";
import { UpdateBanner } from "./shell/UpdateBanner";
import { Dashboard, Wallet, Node, Storage, Comms, Commissary, Settings, Journal, ALF } from "./surfaces";
// CX surfaces (planset citrate-core-social) — scaffold shells wired in CX-S0.4.
import { Models, StorageFiles, Groups, Cluster, Train, Agent, Connections } from "./surfaces";

const queryClient = new QueryClient();

// register-per-route map — verbatim from the design (REG). Instrument = dark
// live telemetry; Charter = light civic/document.
export const REGISTER: Record<string, "instrument" | "charter"> = {
  dashboard: "instrument",
  node: "instrument",
  wallet: "instrument",
  storage: "instrument",
  journal: "charter",
  comms: "charter",
  commissary: "charter",
  settings: "charter",
  // CX surfaces (CX-S0.4)
  models: "instrument",
  files: "instrument",
  groups: "charter",
  cluster: "instrument",
  train: "instrument",
  agent: "charter",
  connections: "charter",
};

function Shell({ s }: { s: AppState }) {
  const reg = REGISTER[s.route] || "charter";

  // banner (entitlement)
  const bannerShow = s.entitlement !== "active";
  let bannerBg = "";
  let bannerBd = "";
  let bannerFg = "";
  let bannerText = "";
  const bannerCta = s.entitlement !== "grace";
  if (s.entitlement === "expiring") {
    bannerBg = "var(--warn-bg)";
    bannerBd = "var(--warn)";
    bannerFg = "var(--warn)";
    bannerText = "Membership renews in 14 days — 2026-07-25. Auto-renewal is off.";
  } else if (s.entitlement === "grace") {
    bannerBg = "var(--warn-bg)";
    bannerBd = "var(--warn)";
    bannerFg = "var(--warn)";
    bannerText = "Cannot verify membership — offline. Paid features stay unlocked for 63 h more. Your node, local wallet, and local memory never lock.";
  } else if (s.entitlement === "lapsed") {
    bannerBg = "var(--danger-bg)";
    bannerBd = "var(--danger)";
    bannerFg = "var(--danger)";
    bannerText = "Membership lapsed. Paid features are locked. Your node, local wallet, and local memory keep working — your keys are yours.";
  }

  const surface = (() => {
    switch (s.route) {
      case "dashboard":
        return <Dashboard store={store} s={s} />;
      case "wallet":
        return <Wallet store={store} s={s} />;
      case "node":
        return <Node store={store} s={s} />;
      case "storage":
        return <Storage store={store} s={s} />;
      case "comms":
        return <Comms store={store} s={s} />;
      case "commissary":
        return <Commissary store={store} s={s} />;
      case "settings":
        return <Settings store={store} s={s} />;
      case "journal":
        return <Journal store={store} s={s} />;
      case "alf":
        return <ALF store={store} s={s} />;
      // CX surfaces (CX-S0.4 shells; lanes fill them in their own files)
      case "models":
        return <Models store={store} s={s} />;
      case "files":
        return <StorageFiles store={store} s={s} />;
      case "groups":
        return <Groups store={store} s={s} />;
      case "cluster":
        return <Cluster store={store} s={s} />;
      case "train":
        return <Train store={store} s={s} />;
      case "agent":
        return <Agent store={store} s={s} />;
      case "connections":
        return <Connections store={store} s={s} />;
      default:
        return <Dashboard store={store} s={s} />;
    }
  })();

  return (
    <div style={{ height: "100vh", display: "grid", gridTemplateColumns: "226px 1fr", overflow: "hidden" }}>
      <Sidebar store={store} s={s} />
      <main style={{ minWidth: 0, minHeight: 0, display: "flex", flexDirection: "column", background: "var(--srf-0)", color: "var(--tx-1)" }} data-register={reg}>
        {bannerShow && (
          <div style={{ display: "flex", alignItems: "center", gap: 12, padding: "9px 26px", background: bannerBg, borderBottom: "1px solid " + bannerBd }}>
            <span style={{ width: 7, height: 7, borderRadius: 999, background: bannerBd, flexShrink: 0 }}></span>
            <span style={{ fontSize: 12.5, color: bannerFg, flex: 1 }}>{bannerText}</span>
            {bannerCta && (
              <button className="btn btn-ghost btn-sm" onClick={() => store.toast("Renewal opens the checkout in your browser — flip entitlement back in the Prototype panel.")}>
                Renew
              </button>
            )}
          </div>
        )}
        <div style={{ flex: 1, minHeight: 0, overflow: "auto" }}>{surface}</div>
      </main>
    </div>
  );
}

/**
 * The launch auth gate (CORE-A3 security). In a Tauri build the app must NEVER
 * render an authenticated surface (the Shell, or any post-sign-in onboarding
 * stage) for a signed-out session — no session is persisted across launches
 * (HIPAA sign-out-by-default), so every launch lands here until a real
 * authority sign-in folds a live claim. This closes the hole where a persisted
 * `stage: "done"` booted straight into the Shell showing the sim persona.
 */
function AuthGate() {
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const signIn = async () => {
    setBusy(true);
    setErr(null);
    try {
      await store.authLogin();
      // On success `signedIn` flips true and Root re-renders past this gate.
    } catch {
      setErr("Sign-in was cancelled or could not be completed. Please try again.");
    } finally {
      setBusy(false);
    }
  };
  return (
    <div
      data-register="charter"
      className="lattice-dots"
      style={{ height: "100vh", display: "flex", flexDirection: "column", alignItems: "center", justifyContent: "center", gap: 24, background: "var(--srf-0)", padding: "48px 24px", textAlign: "center" }}
    >
      <div style={{ width: 140, height: 140 }}>
        <LoaderMark size={140} />
      </div>
      <img src={marqueeBlack} alt="Citrate" style={{ height: 18 }} />
      <div style={{ maxWidth: 460, display: "flex", flexDirection: "column", gap: 12, alignItems: "center" }}>
        <div style={{ fontFamily: "var(--font-display)", fontWeight: 400, fontSize: 28, lineHeight: 1.12, letterSpacing: "-0.011em", color: "var(--tx-1)" }}>
          Sign in to continue
        </div>
        <p style={{ fontSize: 14.5, lineHeight: 1.6, color: "var(--tx-2)", margin: 0 }}>
          Citrate Core signs out on every launch. Confirm it's you — your system browser opens auth.citrate.ai
          (passkey, email, Google, or sign-in-with-Ethereum). Credentials never touch this app.
        </p>
      </div>
      {!busy && (
        <button className="btn btn-primary btn-lg" onClick={signIn}>
          Continue in your browser
        </button>
      )}
      {busy && (
        <div className="surface" style={{ display: "flex", alignItems: "center", gap: 16, padding: "16px 20px" }}>
          <div style={{ width: 40, height: 40, flexShrink: 0 }}>
            <LoaderMark size={40} />
          </div>
          <div style={{ display: "flex", flexDirection: "column", gap: 3, textAlign: "left" }}>
            <div style={{ fontSize: 14, fontWeight: 500 }}>Waiting for your browser…</div>
            <div className="mono" style={{ fontSize: 11, color: "var(--tx-3)" }}>auth.citrate.ai · loopback PKCE</div>
          </div>
        </div>
      )}
      {err && <div style={{ fontSize: 12.5, color: "var(--danger)", maxWidth: 420 }}>{err}</div>}
      <div className="mono" style={{ fontSize: 10, letterSpacing: ".1em", textTransform: "uppercase", color: "var(--tx-3)" }}>
        Chain 40204 · testnet-beta · session never stored on disk
      </div>
    </div>
  );
}

function Root() {
  const s = useStore();

  useEffect(() => {
    store.start();
    // In a Tauri build, hydrate app config from the real on-disk store so the
    // Settings surface reflects persisted values across restart (CORE-A1 A1.4).
    // In sim mode this reads back the current AppState and is a no-op.
    if (bridge.mode === "tauri") {
      bridge.config
        .read()
        .then((cfg) => store.setState(cfg as Partial<AppState>))
        .catch(() => {
          /* honest: a failed read leaves defaults; no fabricated values */
        });
    }
    const onHash = () => {
      const r = (location.hash || "").replace(/^#\//, "");
      if (r && r !== store.state.route && ["dashboard", "wallet", "node", "storage", "journal", "comms", "commissary", "settings", "alf", "models", "files", "groups", "cluster", "train", "agent", "connections"].indexOf(r) >= 0) {
        store.setState({ route: r });
      }
    };
    window.addEventListener("hashchange", onHash);
    onHash();
    return () => {
      store.stop();
      window.removeEventListener("hashchange", onHash);
    };
  }, []);

  // Tauri security gate: a signed-out session may only ever see the welcome
  // (s0) or the sign-in step (s1) of onboarding — never the Shell or any
  // post-sign-in stage (s2..s6/done). Anything past sign-in requires a live
  // authority session. In web-dev (sim) there is no real auth, so no gate.
  const needsAuth =
    bridge.mode === "tauri" && !s.signedIn && s.stage !== "s0" && s.stage !== "s1";

  const body = needsAuth ? (
    <AuthGate />
  ) : s.stage !== "done" ? (
    <Onboarding store={store} s={s} />
  ) : (
    <Shell s={s} />
  );

  return (
    <div style={{ fontFamily: "var(--font-sans)", color: "var(--tx-1)", height: "100vh", overflow: "hidden", background: "var(--srf-0)" }} data-register="charter">
      {body}
      <SignatureCeremony store={store} s={s} />
      <WalletReviewModal store={store} s={s} />
      <Coach store={store} s={s} />
      <DemoPanel store={store} s={s} />
      <Toast s={s} />
      {/* W2.4 — non-blocking in-app update affordance (Tauri only; invisible in sim). */}
      <UpdateBanner />
    </div>
  );
}

function App() {
  return (
    <WagmiProvider config={wagmiConfig}>
      <QueryClientProvider client={queryClient}>
        <Root />
      </QueryClientProvider>
    </WagmiProvider>
  );
}

export default App;
