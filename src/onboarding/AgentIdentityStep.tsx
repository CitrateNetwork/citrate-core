// =====================================================================
// citrate-core — onboarding: give Hermes an identity (HUP-S7.4, US-7.1)
//
// AC1: an AgentSBT is minted at onboarding (HIC-1), bound to the member. The member presses
// one button; core builds the `mintAgent` tx and opens it in the Signature Ceremony, where the
// member approves it. AC2 (visible in Wallet) reuses `RegisteredAgents` below.
//
// The button is enabled ONLY when core says the mint is available: the address book has
// AgentSBT, it has code, the parent organization is active, this node has its key, and a
// preflight of the exact tx succeeds. On chain 40204 today it is not, so the step shows
// "available after the network upgrade" and stays disabled (Rule 1).
// =====================================================================
import { useEffect, type CSSProperties } from "react";
import type { Store } from "../shell/store";
import { agentSbtSlice, defaultAgentSbtIo, identityCard, loadAgentSbt, requestAgentSbtMint, type IdentityCard } from "../identity/agentSbt";
import { BRIDGE_MODE } from "../bridge/mode";

const eyebrow: CSSProperties = {
  fontFamily: "var(--font-mono)",
  fontSize: 11,
  fontWeight: 500,
  letterSpacing: ".14em",
  textTransform: "uppercase",
  color: "var(--tx-3)",
};
const note: CSSProperties = { fontSize: 11.5, color: "var(--tx-3)", lineHeight: 1.5 };

/** The token rows (shared by the onboarding step and the Wallet's registered agents). */
export function AgentTokenRows({ card }: { card: IdentityCard }) {
  return (
    <>
      {card.tokens.map((t) => (
        <div key={t.label} style={{ display: "flex", alignItems: "center", gap: 12, padding: "8px 0" }}>
          <span style={{ fontSize: 12.5, fontWeight: 500, flex: 1 }}>{t.label}</span>
          <span className="mono" style={{ fontSize: 11, color: "var(--tx-3)" }}>
            {t.detail}
            {t.quarantined ? " · quarantined" : ""}
          </span>
        </div>
      ))}
      {card.note && <div style={note}>{card.note}</div>}
    </>
  );
}

export function AgentIdentityView({ card, busy, onMint }: { card: IdentityCard; busy: boolean; onMint: () => void }) {
  const color = card.tone === "error" ? "var(--danger)" : card.tone === "done" ? "var(--ok)" : "var(--tx-2)";
  return (
    <div className="surface" style={{ padding: 20, display: "flex", flexDirection: "column", gap: 12, marginTop: 4 }} data-testid="agent-identity-step">
      <div style={eyebrow}>S6.6 · Hermes identity</div>
      <div style={{ display: "flex", alignItems: "baseline", justifyContent: "space-between", gap: 12 }}>
        <div style={{ fontSize: 15, fontWeight: 500 }}>{card.title}</div>
        <span className="mono" style={{ fontSize: 10, letterSpacing: ".1em", textTransform: "uppercase", color: "var(--tx-3)" }}>
          optional · you approve it
        </span>
      </div>
      <p style={{ fontSize: 13.5, lineHeight: 1.6, color, margin: 0 }} data-testid="agent-identity-message">
        {card.body}
      </p>
      <AgentTokenRows card={card} />
      {card.tone !== "done" && (
        <div style={{ display: "flex", gap: 10 }}>
          <button className="btn btn-primary" disabled={!card.canMint || busy} onClick={onMint} data-testid="agent-identity-mint">
            {busy ? "Preparing the ceremony…" : card.button}
          </button>
        </div>
      )}
      <div className="mono" style={{ fontSize: 10, letterSpacing: ".1em", textTransform: "uppercase", color: "var(--tx-3)" }}>
        Data source: AgentSBT on chain 40204 (balanceOf, mint logs, getAgent) · address from the app's address book
      </div>
    </div>
  );
}

/** Wire the view to core: read the status on mount; the button opens the ceremony. */
export function AgentIdentityStep({ store }: { store: Store }) {
  const st = agentSbtSlice.use();
  useEffect(() => {
    void defaultAgentSbtIo().then(loadAgentSbt);
  }, []);
  const card = identityCard(st.status, { mode: BRIDGE_MODE, loaded: st.loaded, error: st.error });
  const mint = async () => {
    agentSbtSlice.set({ busy: true });
    try {
      const io = await defaultAgentSbtIo();
      const view = await requestAgentSbtMint(io);
      store.openWalletReview("agent", "Give Hermes an identity · AgentSBT mint", view, "network gas only", async () => {
        await loadAgentSbt(await defaultAgentSbtIo());
      });
    } catch (e) {
      store.toast("Hermes identity not available: " + (e instanceof Error ? e.message : String(e)));
      await loadAgentSbt(await defaultAgentSbtIo());
    } finally {
      agentSbtSlice.set({ busy: false });
    }
  };
  return <AgentIdentityView card={card} busy={st.busy} onMint={() => void mint()} />;
}

/** The Wallet's "Registered agents" body for a real member: the AgentSBTs core read, or the
 *  honest state when there are none. */
export function RegisteredAgents() {
  const st = agentSbtSlice.use();
  useEffect(() => {
    void defaultAgentSbtIo().then(loadAgentSbt);
  }, []);
  const card = identityCard(st.status, { mode: BRIDGE_MODE, loaded: st.loaded, error: st.error });
  return (
    <div style={{ padding: "10px 16px" }} data-testid="registered-agents">
      {card.tokens.length > 0 ? (
        <AgentTokenRows card={card} />
      ) : (
        <>
          <p style={{ fontSize: 12.5, color: "var(--tx-3)", margin: 0 }}>{card.body}</p>
          {card.note && <div style={note}>{card.note}</div>}
        </>
      )}
    </div>
  );
}
