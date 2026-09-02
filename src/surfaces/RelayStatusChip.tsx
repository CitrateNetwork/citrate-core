// Flag-A — the relay-status chip. A small, honest indicator of the comms relay link so a connection
// problem is VISIBLE (a dot + label) instead of a silent freeze. Polls bridge.groups.relayStatus()
// (a bounded, read-only backend probe) every few seconds. States come straight from the daemon —
// never fabricated (Rule 1): idle / local / connecting / connected / degraded / error.
import { useEffect, useState } from "react";
import { bridge } from "../bridge";

type RelayState = "idle" | "local" | "connecting" | "connected" | "degraded" | "error";

const META: Record<RelayState, { label: string; color: string; pulse?: boolean; title: string }> = {
  idle: { label: "Relay idle", color: "var(--fg-3, #8a8f88)", title: "Comms not started yet this session." },
  local: { label: "Local relay", color: "var(--info, #4cc3d5)", title: "In-process relay (networked relay disabled)." },
  connecting: { label: "Connecting…", color: "var(--warn, #ffbd10)", pulse: true, title: "Reaching the comms relay…" },
  connected: { label: "Relay connected", color: "var(--citrate-green, #8ecc09)", title: "Connected to the comms relay." },
  degraded: { label: "Relay degraded", color: "var(--danger, #dd7259)", pulse: true, title: "Relay link is down — messages may not deliver until it reconnects." },
  error: { label: "Relay error", color: "var(--danger, #dd7259)", title: "Could not read the relay status." },
};

/** Poll interval for the relay probe (ms). Bounded server-side (≤750ms), so this is cheap. */
const POLL_MS = 5000;

export function RelayStatusChip({ compact = false }: { compact?: boolean }) {
  const [state, setState] = useState<RelayState>("idle");

  useEffect(() => {
    let alive = true;
    const read = async () => {
      try {
        const s = (await bridge.groups.relayStatus()) as RelayState;
        if (alive) setState(META[s] ? s : "connecting");
      } catch {
        if (alive) setState("error");
      }
    };
    read();
    const id = setInterval(read, POLL_MS);
    return () => {
      alive = false;
      clearInterval(id);
    };
  }, []);

  const m = META[state];
  return (
    <span
      title={m.title}
      style={{
        display: "inline-flex",
        alignItems: "center",
        gap: 6,
        fontFamily: "var(--font-sans)",
        fontSize: 11.5,
        color: "var(--fg-2)",
        padding: compact ? 0 : "2px 8px",
        border: compact ? "none" : "1px solid var(--line-1)",
        borderRadius: 999,
        whiteSpace: "nowrap",
      }}
    >
      <span
        aria-hidden="true"
        style={{
          width: 7,
          height: 7,
          borderRadius: 999,
          background: m.color,
          boxShadow: `0 0 0 2px color-mix(in srgb, ${m.color} 22%, transparent)`,
          animation: m.pulse ? "relayPulse 1.4s ease-in-out infinite" : undefined,
        }}
      />
      {!compact && <span>{m.label}</span>}
      <style>{`@keyframes relayPulse { 0%,100%{opacity:1} 50%{opacity:.35} }`}</style>
    </span>
  );
}
