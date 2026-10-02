import React from "react";
import ReactDOM from "react-dom/client";
import { ErrorBoundary } from "./ErrorBoundary";
import { popoutKindFromLabel } from "./popout/kinds";
import "./styles/index.css";

/** HUP-S5.4 — this window's Tauri label (injected by Tauri; absent in the web preview and tests). */
function windowLabel(): string {
  const internals = (window as unknown as { __TAURI_INTERNALS__?: { metadata?: { currentWindow?: { label?: unknown } } } })
    .__TAURI_INTERNALS__;
  const label = internals?.metadata?.currentWindow?.label;
  return typeof label === "string" ? label : "main";
}

const rootEl = document.getElementById("root") as HTMLElement;
const popout = popoutKindFromLabel(windowLabel());

if (popout) {
  // HUP-S5.4: a pop-out window renders only its own view. It never imports the app store, so it
  // starts no polling and never writes the member's saved state.
  void import("./popout/PopoutRoot").then(({ PopoutRoot }) =>
    import("./popout/bridge").then(({ tauriTransport }) => {
      ReactDOM.createRoot(rootEl).render(
        <React.StrictMode>
          <ErrorBoundary>
            <PopoutRoot kind={popout} transport={tauriTransport} />
          </ErrorBoundary>
        </React.StrictMode>,
      );
    }),
  );
} else {
  void Promise.all([import("./App"), import("./shell/store"), import("./popout/appHost"), import("./daemons/appRunner")]).then(
    ([{ default: App }, { store }, { startPopoutHost }, { startDaemonRunner }]) => {
      // DEV-only test seam: expose the app store so the docs screenshot harness
      // (screenshots/) can drive onboarding stages and shell routes deterministically
      // while the app runs in sim mode (`vite dev`, no Tauri, no node). `import.meta.env.DEV`
      // is compiled out of production/Tauri builds, so this never ships.
      if (import.meta.env.DEV) {
        (window as unknown as { __citrateStore?: typeof store }).__citrateStore = store;
      }
      // HUP-S5.4: the main window hosts the pop-outs (desktop app only; a no-op in the preview).
      startPopoutHost()?.catch((e) => console.error("pop-out host", e));
      // HUP-S10.3: the daemon runner (desktop app only; nothing runs until the member creates one).
      startDaemonRunner();
      ReactDOM.createRoot(rootEl).render(
        <React.StrictMode>
          <ErrorBoundary>
            <App />
          </ErrorBoundary>
        </React.StrictMode>,
      );
    },
  );
}
