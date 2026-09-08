import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { store } from "./shell/store";
import "./styles/index.css";

// DEV-only test seam: expose the app store so the docs screenshot harness
// (screenshots/) can drive onboarding stages and shell routes deterministically
// while the app runs in sim mode (`vite dev`, no Tauri, no node). `import.meta.env.DEV`
// is compiled out of production/Tauri builds, so this never ships.
if (import.meta.env.DEV) {
  (window as unknown as { __citrateStore?: typeof store }).__citrateStore = store;
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
