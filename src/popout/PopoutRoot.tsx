// =====================================================================
// citrate-core — the root of a pop-out window (HUP-S5.4)
//
// A pop-out runs this instead of the app. It never builds the app store (which would start the
// app's polling and write the member's saved state): it only talks to the main window over the
// typed bridge. Its capability grants no app commands, so the bridge is all it can use.
// HUP-S10.6 (a11y): the document is titled after the pop-out, and every state (not built,
// waiting, failed) is a <main> landmark with an <h1>, with a polite status or an alert.
// =====================================================================
import { useEffect, useId, useRef, useState, type ReactNode } from "react";
import { createPopoutEnd, type BridgeTransport, type PopoutEnd } from "./bridge";
import { POPOUT_TITLES, type PopoutKind } from "./kinds";
import type { MonitorSnapshot } from "./monitorSnapshot";
import { ActivityMonitor } from "./ActivityMonitor";

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

export function PopoutRoot({ kind, transport }: { kind: PopoutKind; transport: () => Promise<BridgeTransport> }) {
  const [snapshot, setSnapshot] = useState<MonitorSnapshot | null>(null);
  const [failed, setFailed] = useState<string | null>(null);
  const [now, setNow] = useState(() => Date.now());
  const end = useRef<PopoutEnd | null>(null);

  // Screen readers announce the document title for the window; name it after the pop-out.
  useEffect(() => {
    document.title = POPOUT_TITLES[kind];
  }, [kind]);

  useEffect(() => {
    if (kind !== "monitor") return;
    let cancelled = false;
    void (async () => {
      try {
        const t = await transport();
        const e = await createPopoutEnd(t, kind, (s) => setSnapshot(s));
        if (cancelled) {
          e.close();
          return;
        }
        end.current = e;
        await e.ready();
      } catch (err) {
        if (!cancelled) setFailed(err instanceof Error ? err.message : String(err));
      }
    })();
    return () => {
      cancelled = true;
      end.current?.close();
      end.current = null;
    };
  }, [kind, transport]);

  // The elapsed clock ticks here; the snapshot carries the real start time.
  useEffect(() => {
    if (kind !== "monitor") return;
    const t = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(t);
  }, [kind]);

  if (kind !== "monitor") {
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
          The Activity monitor could not connect to the main window: {failed}
        </p>
      </Frame>
    );
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
  return <ActivityMonitor snapshot={snapshot} now={now} onStop={() => void end.current?.stop()} />;
}
