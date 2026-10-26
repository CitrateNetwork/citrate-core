// K1a-2 — Node › Services (red stub: the registry rows render, no states yet).
import type { Store } from "../shell/store";
import type { ServiceId, ServicePhase, ServicesSnapshot } from "../bridge/domains";

export const PHASE_LABEL: Record<ServicePhase, string> = {
  stopped: "Stopped",
  starting: "Starting",
  alive: "Alive",
  backoff: "Restarting after a crash",
  failed: "Failed",
  stopping: "Stopping",
  quarantined: "Quarantined",
};

export const OBSERVATION_LABEL = {
  notSupervised: "Not supervised",
  busy: "Unknown",
} as const;

export type ControlAction = "start" | "stop";

export const SERVICE_CONTROLS: Partial<Record<ServiceId, (store: Store) => Record<ControlAction, () => Promise<void>>>> = {};

export function ServicesView(props: {
  snapshot: ServicesSnapshot | null;
  error: string | null;
  pending: Partial<Record<ServiceId, ControlAction>>;
  onControl: (id: ServiceId, action: ControlAction) => void;
  onOpenLogs: () => void;
}) {
  return (
    <div>
      {props.snapshot?.services.map((e) => (
        <div key={e.id} data-service={e.id}>
          {e.label}
        </div>
      ))}
    </div>
  );
}
