// =====================================================================
// citrate-core — the root of a pop-out window (HUP-S5.4)
//
// A pop-out runs this instead of the app. It never builds the app store (which would start the
// app's polling and write the member's saved state): it only talks to the main window over the
// typed bridge. Its capability grants no app commands, so the bridge is all it can use.
// =====================================================================
import { useEffect, useRef, useState } from "react";
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

export function PopoutRoot({ kind, transport }: { kind: PopoutKind; transport: () => Promise<BridgeTransport> }) {
  const [snapshot, setSnapshot] = useState<MonitorSnapshot | null>(null);
  const [failed, setFailed] = useState<string | null>(null);
  const [now, setNow] = useState(() => Date.now());
  const end = useRef<PopoutEnd | null>(null);

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
      <div data-register="instrument" style={shell}>
        The {POPOUT_TITLES[kind]} pop-out is not built yet.
      </div>
    );
  }
  if (failed) {
    return (
      <div data-register="instrument" role="alert" style={shell}>
        The Activity monitor could not connect to the main window: {failed}
      </div>
    );
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
