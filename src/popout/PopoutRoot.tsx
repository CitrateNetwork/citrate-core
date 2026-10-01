// =====================================================================
// citrate-core — the root of a pop-out window (HUP-S5.4)
//
// A pop-out runs this instead of the app. It never builds the app store (which would start the
// app's polling and write the member's saved state): it only talks to the main window over the
// typed bridge. Its capability grants no app commands, so the bridge is all it can use.
// HUP-S5.1: the Browser pop-out renders the views the main window sends, re-announces itself every
// few seconds (the main window polls the browser only while it hears from it), and its Stop asks the
// main window to stop Hermes's browser.
// =====================================================================
import { useEffect, useRef, useState } from "react";
import { createPopoutEnd, type BridgeTransport, type PopoutEnd } from "./bridge";
import { POPOUT_TITLES, type PopoutKind } from "./kinds";
import type { MonitorSnapshot } from "./monitorSnapshot";
import { ActivityMonitor } from "./ActivityMonitor";
import { BrowserPopout } from "./BrowserPopout";
import type { BrowserView } from "./browserView";

/** How often a Browser pop-out re-announces itself to the main window. */
export const BROWSER_HEARTBEAT_MS = 3_000;

const shell = {
  minHeight: "100vh",
  boxSizing: "border-box" as const,
  padding: "14px 16px",
  background: "var(--srf-0)",
  color: "var(--tx-1)",
  fontFamily: "var(--font-sans)",
  fontSize: 13,
};

export function PopoutRoot({ kind, transport }: { kind: PopoutKind; transport: () => Promise<BridgeTransport> }) {
  const [snapshot, setSnapshot] = useState<MonitorSnapshot | null>(null);
  const [browserView, setBrowserView] = useState<BrowserView | null>(null);
  const [failed, setFailed] = useState<string | null>(null);
  const [now, setNow] = useState(() => Date.now());
  const end = useRef<PopoutEnd | null>(null);

  const hasView = kind === "monitor" || kind === "browser";

  useEffect(() => {
    if (!hasView) return;
    let cancelled = false;
    let heartbeat: ReturnType<typeof setInterval> | null = null;
    void (async () => {
      try {
        const t = await transport();
        const e = await createPopoutEnd(t, kind, (s) => setSnapshot(s), (v) => setBrowserView(v));
        if (cancelled) {
          e.close();
          return;
        }
        end.current = e;
        await e.ready();
        if (kind === "browser" && !cancelled) {
          heartbeat = setInterval(() => void e.ready().catch(() => undefined), BROWSER_HEARTBEAT_MS);
        }
      } catch (err) {
        if (!cancelled) setFailed(err instanceof Error ? err.message : String(err));
      }
    })();
    return () => {
      cancelled = true;
      if (heartbeat !== null) clearInterval(heartbeat);
      end.current?.close();
      end.current = null;
    };
  }, [kind, transport, hasView]);

  // The elapsed clock ticks here; the snapshot carries the real start time.
  useEffect(() => {
    if (kind !== "monitor") return;
    const t = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(t);
  }, [kind]);

  if (!hasView) {
    return (
      <div data-register="instrument" style={shell}>
        The {POPOUT_TITLES[kind]} pop-out is not built yet.
      </div>
    );
  }
  if (failed) {
    return (
      <div data-register="instrument" role="alert" style={shell}>
        The {POPOUT_TITLES[kind]} could not connect to the main window: {failed}
      </div>
    );
  }
  if (kind === "browser") {
    if (!browserView) {
      return (
        <div data-register="instrument" style={shell}>
          Waiting for the main window…
        </div>
      );
    }
    return <BrowserPopout view={browserView} onStop={() => void end.current?.stopBrowser()} />;
  }
  if (!snapshot) {
    return (
      <div data-register="instrument" style={shell}>
        Waiting for the main window…
      </div>
    );
  }
  return <ActivityMonitor snapshot={snapshot} now={now} onStop={() => void end.current?.stop()} />;
}
