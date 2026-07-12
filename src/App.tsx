import { useEffect } from "react";
import { WagmiProvider } from "wagmi";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { wagmiConfig } from "./wagmi";
import { store, useStore } from "./shell/store";
import { AppState } from "./shell/state";
import { Onboarding } from "./onboarding/Onboarding";
import { Sidebar } from "./shell/Sidebar";
import { SignatureCeremony, Coach, Toast, DemoPanel } from "./shell/Chrome";
import { Dashboard } from "./surfaces/Dashboard";
import { Wallet, Node, Storage, Comms, Commissary, Settings, Journal } from "./surfaces/stubs";

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

function Root() {
  const s = useStore();

  useEffect(() => {
    store.start();
    const onHash = () => {
      const r = (location.hash || "").replace(/^#\//, "");
      if (r && r !== store.state.route && ["dashboard", "wallet", "node", "storage", "journal", "comms", "commissary", "settings"].indexOf(r) >= 0) {
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

  return (
    <div style={{ fontFamily: "var(--font-sans)", color: "var(--tx-1)", height: "100vh", overflow: "hidden", background: "var(--srf-0)" }} data-register="charter">
      {s.stage !== "done" ? <Onboarding store={store} s={s} /> : <Shell s={s} />}
      <SignatureCeremony store={store} s={s} />
      <Coach store={store} s={s} />
      <DemoPanel store={store} s={s} />
      <Toast s={s} />
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
