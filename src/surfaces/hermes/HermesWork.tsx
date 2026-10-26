// =====================================================================
// K2 (0.5.2): Hermes › Work. Agents, skills, "Awaiting your review", daemons and the run log in one
// tab, composed from what exists: the Agent surface (roster, skills, approvals rail, contracts),
// the Daemons card (daemons_list, daemon_set_paused, daemons_set_all_paused, daemon_save) and the
// daemon run log (daemon_runs_between). "Approval records", "Full history" and each daemon's "Runs"
// open Hermes › Controls › 3 · Records on the matching view.
// =====================================================================
import { SurfaceProps } from "../shared";
import { Agent } from "../Agent";
import { DaemonsPanel } from "../../daemons/DaemonsPanel";
import { DaemonRunLog } from "../../daemons/DaemonRunLog";
import { openRecords } from "./ControlsRecords";

export function HermesWork({ store, s }: SurfaceProps) {
  const toRuns = (daemonId: string | null) => {
    openRecords("runs", daemonId);
    store.go("hermes/controls/records");
  };
  return (
    <Agent store={store} s={s}>
      <DaemonsPanel onRuns={(id) => toRuns(id)} />
      <DaemonRunLog onFullHistory={() => toRuns(null)} />
    </Agent>
  );
}
