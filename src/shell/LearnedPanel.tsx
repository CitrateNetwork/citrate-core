// HUP-S3.4 — "What Hermes learned": proposals waiting for the member, saved skills, and learned
// memories with their Belnap state. Reads and decides through bridge.agentHarness; nothing here
// persists on its own (the sidecar records each decision first, core stores memories).
import { useCallback, useEffect, useState } from "react";
import { bridge } from "../bridge";
import { acknowledgedFor, memoryRowModel, type LearnedMemory, type LearnProposal, type LearnStatus } from "../agent/learn";
import { LearnProposalCard } from "./LearnProposalCard";

interface Toaster {
  toast(msg: string): void;
}

export interface LearnedView {
  status: LearnStatus | null;
  proposals: LearnProposal[];
  memories: LearnedMemory[];
  error: string | null;
}

const awaiting = (p: LearnProposal) => p.state.state === "proposed" || p.state.state === "persist_failed";
const savedSkill = (p: LearnProposal) => p.kind === "skill" && (p.state.state === "persisted" || p.state.state === "publish_prepared");

/** The panel body for a loaded view (rendered by tests without the bridge). */
export function LearnedPanelView({
  view,
  acked,
  busy,
  onAck,
  onAccept,
  onReject,
  onPublish,
  onStorePending,
}: {
  view: LearnedView;
  acked: Record<string, ReadonlySet<string>>;
  busy: boolean;
  onAck(id: string, conflict: string, on: boolean): void;
  onAccept(p: LearnProposal): void;
  onReject(p: LearnProposal, reason: string): void;
  onPublish(p: LearnProposal): void;
  onStorePending(): void;
}) {
  const enabled = view.status?.sidecar.enabled === true;
  const waiting = view.proposals.filter(awaiting);
  const skills = view.proposals.filter(savedSkill);
  const publish = view.status?.publish ?? null;
  const pendingGraph = view.memories.some((m) => m.graph.state !== "stored");
  return (
    <div data-testid="learned-panel" className="surface" style={{ display: "flex", flexDirection: "column" }}>
      <div style={{ display: "flex", alignItems: "center", padding: "12px 16px", borderBottom: "1px solid var(--line-1)" }}>
        <span style={{ fontSize: 13.5, fontWeight: 500 }}>What Hermes learned</span>
        <span className="mono" style={{ marginLeft: "auto", fontSize: 10, color: "var(--tx-3)" }}>
          only from verified work · nothing is kept without you
        </span>
      </div>
      <div style={{ padding: "12px 16px", display: "flex", flexDirection: "column", gap: 10 }}>
        {view.error && (
          <span data-testid="learned-error" style={{ fontSize: 12, color: "var(--danger)" }}>
            {view.error}
          </span>
        )}
        {!enabled && (
          <span data-testid="learned-off" style={{ fontSize: 12, color: "var(--tx-3)" }}>
            {view.status?.sidecar.error ? `Learning is not available: ${view.status.sidecar.error}.` : "Learning is not available until Hermes is running."}
          </span>
        )}
        {enabled && waiting.length === 0 && (
          <span data-testid="learned-empty" style={{ fontSize: 12, color: "var(--tx-3)" }}>
            No proposals. Hermes proposes a skill or a memory only after a workflow whose checks all passed.
          </span>
        )}
        {waiting.map((p) => (
          <LearnProposalCard
            key={p.id}
            proposal={p}
            publish={publish}
            acked={acked[p.id] ?? new Set()}
            busy={busy}
            onAck={(c, on) => onAck(p.id, c, on)}
            onAccept={() => onAccept(p)}
            onReject={(r) => onReject(p, r)}
          />
        ))}
        {skills.length > 0 && (
          <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
            <span className="mono" style={{ fontSize: 9.5, letterSpacing: ".1em", textTransform: "uppercase", color: "var(--tx-3)" }}>
              Saved skills
            </span>
            {skills.map((p) => (
              <LearnProposalCard key={p.id} proposal={p} publish={publish} acked={new Set()} busy={busy} onAck={() => undefined} onAccept={() => undefined} onReject={() => undefined} onPublish={() => onPublish(p)} />
            ))}
          </div>
        )}
        {view.memories.length > 0 && (
          <div data-testid="learned-memories" style={{ display: "flex", flexDirection: "column", gap: 6 }}>
            <div style={{ display: "flex", alignItems: "center" }}>
              <span className="mono" style={{ fontSize: 9.5, letterSpacing: ".1em", textTransform: "uppercase", color: "var(--tx-3)" }}>
                Learned memories
              </span>
              {pendingGraph && (
                <button className="btn btn-ghost btn-sm" disabled={busy} onClick={onStorePending} style={{ marginLeft: "auto" }}>
                  Store waiting memories
                </button>
              )}
            </div>
            {view.memories.map((mem) => {
              const r = memoryRowModel(mem);
              const color = r.tone === "ok" ? "var(--tx-3)" : r.tone === "warn" ? "var(--warn)" : "var(--danger)";
              return (
                <div key={mem.proposalId} data-testid="learned-memory" data-belnap={mem.belnap} style={{ display: "flex", flexDirection: "column", gap: 1 }}>
                  <span style={{ fontSize: 12.5, color: "var(--tx-1)", overflowWrap: "anywhere" }}>
                    <strong style={{ fontWeight: 500 }}>{r.title}:</strong> {r.value}
                  </span>
                  {r.belnapLabel && <span style={{ fontSize: 11, color: "var(--warn)" }}>{r.belnapLabel}</span>}
                  <span className="mono" style={{ fontSize: 10.5, color }}>
                    {r.graphLabel}
                  </span>
                </div>
              );
            })}
          </div>
        )}
      </div>
    </div>
  );
}

/** The live panel: loads from the bridge on mount and after every decision. */
export function LearnedPanel({ store, running }: { store: Toaster; running: boolean }) {
  const [view, setView] = useState<LearnedView>({ status: null, proposals: [], memories: [], error: null });
  const [acked, setAcked] = useState<Record<string, ReadonlySet<string>>>({});
  const [busy, setBusy] = useState(false);

  const load = useCallback(async () => {
    try {
      const status = await bridge.agentHarness.learnStatus();
      const [proposals, memories] = await Promise.all([
        status.sidecar.enabled ? bridge.agentHarness.learnProposals(true) : Promise.resolve([] as LearnProposal[]),
        bridge.agentHarness.learnMemories(),
      ]);
      setView({ status, proposals, memories, error: null });
    } catch (e) {
      setView((v) => ({ ...v, error: e instanceof Error ? e.message : String(e) }));
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load, running]);

  const act = async (what: () => Promise<string | void>) => {
    setBusy(true);
    try {
      const msg = await what();
      if (msg) store.toast(msg);
    } catch (e) {
      const m = e instanceof Error ? e.message : String(e);
      store.toast(m.replace(/^(LEARN_REFUSED|PUBLISH_DISABLED): /, ""));
    } finally {
      setBusy(false);
      await load();
    }
  };

  return (
    <LearnedPanelView
      view={view}
      acked={acked}
      busy={busy}
      onAck={(id, c, on) =>
        setAcked((a) => {
          const next = new Set(a[id] ?? []);
          if (on) next.add(c);
          else next.delete(c);
          return { ...a, [id]: next };
        })
      }
      onAccept={(p) =>
        void act(async () => {
          const r = await bridge.agentHarness.learnAccept(p.id, acknowledgedFor(p, acked[p.id] ?? new Set()));
          if (r.memory) {
            return r.memory.graph.state === "stored"
              ? r.memory.belnap === "both"
                ? "Memory kept. It contradicts an earlier one, so neither is relied on until you resolve it."
                : "Memory kept in your memory graph."
              : "Memory kept. It goes into your memory graph when the memory store is running.";
          }
          return "Skill saved to your skills. Hermes can use it from its next start.";
        })
      }
      onReject={(p, reason) =>
        void act(async () => {
          await bridge.agentHarness.learnReject(p.id, reason);
          return "Rejected. Nothing was kept.";
        })
      }
      onPublish={(p) =>
        void act(async () => {
          await bridge.agentHarness.learnPublish(p.id, "1.0.0");
          return "Review and sign the publish in the Signature Ceremony.";
        })
      }
      onStorePending={() =>
        void act(async () => {
          await bridge.agentHarness.learnStorePending();
        })
      }
    />
  );
}
