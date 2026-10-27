// =====================================================================
// K2 (0.5.2): Hermes › Learn. One place for what Hermes learns, composed from the existing pieces:
//
//   Hermes running        agent_harness_status via refreshAgent (read on mount)
//
//   What Hermes learned   LearnedPanel (src/shell/LearnedPanel.tsx): proposals waiting for you,
//                         saved skills, learned memories, and Teach Hermes (TeachHermesCard).
//                         hermes_learn_status / proposals / accept / reject / memories,
//                         hermes_workflow_run (learnLauncher.ts)
//   Knowledge import      store.importKnowledge → memory_import_knowledge (also run before the
//                         memory store starts); status as the importer reports it
//   Training opt-in       TrajectoryConsent (src/fl/TrajectoryConsent.tsx):
//                         trajectories_settings_get / trajectories_settings_set
//
// Group rounds are not live on 40204 (UI truth contract §4): recordings stay on this machine.
// =====================================================================
import { SurfaceProps } from "../shared";
import { LearnedPanel } from "../../shell/LearnedPanel";
import { TrajectoryConsent } from "../../fl/TrajectoryConsent";
import { KnowledgeImportStatus } from "../Storage";
import { useEffect } from "react";
import { agentSlice, refreshAgent } from "../../shell/slices/agent";
import { bridge } from "../../bridge";

const para = { fontSize: 12.5, lineHeight: 1.6, color: "var(--tx-2)", margin: 0 } as const;

export function HermesLearn({ store, s }: SurfaceProps) {
  const st = agentSlice.use();
  // Read the harness status here (agent_harness_status): agentSlice starts with Hermes off, and
  // Learn must not depend on the Agent page having been opened first.
  useEffect(() => {
    void refreshAgent();
  }, []);
  const running = st.attached.some((r) => r.id === "hermes" && r.wired && r.state === "running");
  const importing = s.knowledgeImport.state === "running";
  return (
    <div style={{ padding: "20px 26px 24px", display: "grid", gridTemplateColumns: "minmax(0,1fr) minmax(0,1fr)", gap: 16, alignItems: "start", boxSizing: "border-box" }}>
      <div style={{ display: "flex", flexDirection: "column", gap: 16, minWidth: 0 }}>
        <LearnedPanel store={store} running={running} />
      </div>
      <div style={{ display: "flex", flexDirection: "column", gap: 16, minWidth: 0 }}>
        <div className="surface" data-testid="learn-knowledge-import" data-source="memory_import_knowledge" style={{ padding: 18, display: "flex", flexDirection: "column", gap: 10 }}>
          <span style={{ fontSize: 13.5, fontWeight: 500 }}>Knowledge import</span>
          <p style={para}>
            Load the built-in Citrate knowledge (public docs, papers, reviewed skills) into your memory graph on this machine, so Hermes can answer questions about the network from local pages. A finished import is not repeated.
          </p>
          <KnowledgeImportStatus store={store} s={s} />
          <span>
            <button className="btn btn-ghost btn-sm" data-testid="learn-knowledge-import-run" disabled={importing} onClick={() => void store.importKnowledge()}>
              {importing ? "Importing…" : "Import built-in knowledge"}
            </button>
          </span>
        </div>
        <div className="surface" data-testid="learn-training" data-source="trajectories_settings_get / trajectories_settings_set" style={{ padding: 18, display: "flex", flexDirection: "column", gap: 10 }}>
          <span style={{ fontSize: 13.5, fontWeight: 500 }}>Training opt-in</span>
          <TrajectoryConsent fl={bridge.flRounds} toast={(m) => store.toast(m)} />
          <span style={{ display: "flex", alignItems: "center", gap: 8 }}>
            <span style={{ width: 6, height: 6, borderRadius: 999, background: "var(--tx-3)", flexShrink: 0 }}></span>
            <span style={{ fontSize: 12, color: "var(--tx-2)" }}>Group rounds not live yet. Nothing recorded here is sent anywhere.</span>
          </span>
        </div>
      </div>
    </div>
  );
}
