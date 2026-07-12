import { SurfaceProps, StubShell } from "./shared";

export function Comms(_: SurfaceProps) {
  return <StubShell title="Ping center" note="Notification-only ping center — actor, room, kind, and time; message bodies are never stored here. Polls every 20 seconds, honestly." />;
}
