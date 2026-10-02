// =====================================================================
// citrate-core — the root of a pop-out window (HUP-S5.4)
//
// A pop-out runs this instead of the app. It never builds the app store (which would start the
// app's polling and write the member's saved state): it only talks to the main window over the
// typed bridge. Its capability grants no app commands, so the bridge is all it can use.
// HUP-S5.1: the Browser pop-out renders the views the main window sends, re-announces itself every
// few seconds (the main window polls the browser only while it hears from it), and its Stop asks the
// main window to stop Hermes's browser.
// HUP-S10.6 (a11y): the document is titled after the pop-out, and every state (not built,
// waiting, failed) is a <main> landmark with an <h1>, with a polite status or an alert.
// =====================================================================
import { useEffect, useId, useRef, useState, type ReactNode } from "react";
import { createPopoutEnd, type BridgeTransport, type PopoutEnd } from "./bridge";
import { POPOUT_TITLES, type PopoutKind } from "./kinds";
import type { MonitorSnapshot } from "./monitorSnapshot";
import type { UndoPanel } from "./undoPanel";
import { ActivityMonitor } from "./ActivityMonitor";
import { ContractReader } from "./ContractReader";
import { createContractClient, type ContractClient } from "./contractChannel";
import { MediaPlayer } from "./MediaPlayer";
import { mediaTauriTransport } from "./mediaBridge";
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

const para = { margin: 0, color: "var(--tx-1)" };

/** The landmark + heading every non-monitor state renders inside. */
function Frame({ title, children }: { title: string; children: ReactNode }) {
  const id = useId();
  return (
    <main data-register="instrument" aria-labelledby={id} style={{ ...shell, display: "flex", flexDirection: "column", gap: 8 }}>
      <h1 id={id} style={{ fontSize: 14, fontWeight: 500, margin: 0 }}>
        {title}
      </h1>
      {children}
    </main>
  );
}

export function PopoutRoot({
  kind,
  transport,
  mediaTransport = mediaTauriTransport,
}: {
  kind: PopoutKind;
  transport: () => Promise<BridgeTransport>;
  /** HUP-S10.1: the Media player's own channel (injected for tests). */
  mediaTransport?: () => Promise<BridgeTransport>;
}) {
  const [snapshot, setSnapshot] = useState<MonitorSnapshot | null>(null);
  const [undoPanel, setUndoPanel] = useState<UndoPanel | null>(null);
  const [browserView, setBrowserView] = useState<BrowserView | null>(null);
  const [failed, setFailed] = useState<string | null>(null);
  const [now, setNow] = useState(() => Date.now());
  const end = useRef<PopoutEnd | null>(null);

  const hasView = kind === "monitor" || kind === "browser";
  // Screen readers announce the document title for the window; name it after the pop-out.
  useEffect(() => {
    document.title = POPOUT_TITLES[kind];
  }, [kind]);

  useEffect(() => {
    if (!hasView) return;
    let cancelled = false;
    let heartbeat: ReturnType<typeof setInterval> | null = null;
    void (async () => {
      try {
        const t = await transport();
        const e = await createPopoutEnd(
          t,
          kind,
          (s) => setSnapshot(s),
          (p) => setUndoPanel(p),
          (v) => setBrowserView(v),
        );
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

  if (kind === "contract") return <ContractReaderWindow transport={transport} />;
  if (kind === "media") return <MediaPlayer transport={mediaTransport} />;
  if (!hasView) {
    return (
      <Frame title={POPOUT_TITLES[kind]}>
        <p style={para}>The {POPOUT_TITLES[kind]} pop-out is not built yet.</p>
      </Frame>
    );
  }
  if (failed) {
    return (
      <Frame title={POPOUT_TITLES[kind]}>
        <p role="alert" style={para}>
          The {POPOUT_TITLES[kind]} could not connect to the main window: {failed}
        </p>
      </Frame>
    );
  }
  if (kind === "browser") {
    if (!browserView) {
      return (
        <Frame title={POPOUT_TITLES[kind]}>
          <p role="status" aria-live="polite" style={para}>
            Waiting for the main window…
          </p>
        </Frame>
      );
    }
    return <BrowserPopout view={browserView} onStop={() => void end.current?.stopBrowser()} />;
  }
  if (!snapshot) {
    return (
      <Frame title={POPOUT_TITLES[kind]}>
        <p role="status" aria-live="polite" style={para}>
          Waiting for the main window…
        </p>
      </Frame>
    );
  }
  return (
    <ActivityMonitor
      snapshot={snapshot}
      now={now}
      onStop={() => void end.current?.stop()}
      undo={undoPanel}
      onUndo={(session, seq) => void end.current?.undo(session, seq)}
      onPauseDaemon={(id, paused) => void end.current?.pauseDaemon(id, paused)}
      onStopDaemon={() => void end.current?.stopDaemon()}
    />
  );
}

/** HUP-S6.7 — the Contract reader window: its requests go to the main window over the channel. */
function ContractReaderWindow({ transport }: { transport: () => Promise<BridgeTransport> }) {
  const [client, setClient] = useState<ContractClient | null>(null);
  const [failed, setFailed] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    let made: ContractClient | null = null;
    void (async () => {
      try {
        const c = await createContractClient(await transport());
        if (cancelled) {
          c.close();
          return;
        }
        made = c;
        setClient(c);
      } catch (err) {
        if (!cancelled) setFailed(err instanceof Error ? err.message : String(err));
      }
    })();
    return () => {
      cancelled = true;
      made?.close();
    };
  }, [transport]);

  if (failed) {
    return (
      <Frame title={POPOUT_TITLES.contract}>
        <p role="alert" style={para}>
          The Contract reader could not connect to the main window: {failed}
        </p>
      </Frame>
    );
  }
  if (!client) {
    return (
      <Frame title={POPOUT_TITLES.contract}>
        <p role="status" aria-live="polite" style={para}>
          Connecting to the main window…
        </p>
      </Frame>
    );
  }
  return <ContractReader client={client} />;
}
