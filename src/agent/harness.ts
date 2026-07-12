// =====================================================================
// citrate-core — chat harness (ported 1:1 from design/chat-harness.js)
//
// The Dashboard agent speaks to any provider that implements:
//   provider.send({ messages, callbacks }) -> Promise<assistantMessage>
//
// Production wiring (02 §7): swap createDemoProvider for
// createGatewayProvider (inference-gateway local-proxy, OpenAI-compatible,
// cgk_ bearer) with createLocalProvider (llama-server sidecar) as the
// offline fallback. The message/tool shapes are OpenAI chat-completions
// shapes, so the swap is transport-only.
//
// Data source — the demo provider scripts reasoning over the LIVE sim
// snapshot; the gateway/local providers are the real transport, wired in
// a later wave. This module is the seam, not a mock of chain data.
// =====================================================================

export type ChatStatus = "thinking" | "streaming" | "tool" | "done" | "error";

export interface ToolCall {
  id: string;
  name: string;
  arguments: string;
  result?: string;
}

export interface AgentContext {
  height: number;
  peers: number;
  finalityAge: number;
  nodeState: string;
  staked: number;
  liquid: number;
  claimable: number;
  earningsToday: number;
  walletAddr: string;
  tier: string;
}

export interface SendOpts {
  messages: { role: string; content: string }[];
  callbacks: {
    onStatus: (status: ChatStatus) => void;
    onToken: (text: string) => void;
    onToolCall: (call: ToolCall) => Promise<string>;
  };
}

export interface ChatProvider {
  kind: string;
  label: string;
  send: (opts: SendOpts) => Promise<{ role: string; content: string }>;
}

export const AGENT_SYSTEM_PROMPT = [
  "You are the Citrate member agent inside citrate-core.",
  "You may read the member’s memory graph and local chain state.",
  "Every write (memory_assert, any chain transaction) is proposed, never executed:",
  "writes queue for human approval in the Signature Ceremony.",
  "Speak plainly. Never fabricate numbers; read them through tools.",
].join(" ");

// ---------------------------------------------------------------------
// Demo provider — same contract, scripted reasoning over live sim state.
// ---------------------------------------------------------------------
export function createDemoProvider(getContext: () => AgentContext): ChatProvider {
  return {
    kind: "demo",
    label: "infer.citrate.ai · local-proxy",
    async send({ messages, callbacks }) {
      const userText = (messages[messages.length - 1]?.content || "").toLowerCase();
      callbacks.onStatus("thinking");
      await wait(900 + Math.random() * 700);

      const plan = routeIntent(userText);

      for (const call of plan.toolCalls) {
        callbacks.onStatus("tool");
        await wait(350);
        call.result = await callbacks.onToolCall(call);
      }

      const text = plan.compose(getContext(), plan.toolCalls);
      callbacks.onStatus("streaming");
      let out = "";
      for (const token of tokenize(text)) {
        out += token;
        callbacks.onToken(token);
        await wait(14 + Math.random() * 26);
      }
      callbacks.onStatus("done");
      return { role: "assistant", content: out };
    },
  };
}

interface Plan {
  toolCalls: ToolCall[];
  compose: (c: AgentContext, calls: ToolCall[]) => string;
}

function routeIntent(t: string): Plan {
  const has = (...ws: string[]) => ws.some((w) => t.includes(w));

  if (has("stake", "staked", "position", "validator status")) {
    return {
      toolCalls: [tc("chain_read", { query: "staking.position" })],
      compose: (c) =>
        `You have **${fmt(c.staked)} SALT** staked in the LiquidStakingPool — your full membership grant, vaulted and attributed to your validator. That meets the 32,000 SALT minimum, so your node ${c.nodeState === "validating" ? "is validating and earning" : "is eligible to validate once running"}. The granted principal stays vaulted until mainnet release; rewards your node earns are yours. Withdrawals of self-added stake carry a 7-day lockup.`,
    };
  }
  if (has("earn", "reward", "claim", "income", "made today")) {
    return {
      toolCalls: [tc("chain_read", { query: "earnings.claimable" })],
      compose: (c) =>
        `Today your node has earned **${c.earningsToday.toFixed(2)} SALT** — validation makes up most of it, with smaller pinning and compute shares. Your claimable balance is **${c.claimable.toFixed(2)} SALT**; claims batch until they clear the dust threshold, and claiming signs a claimRewards() transaction through the ceremony. Want me to take you to the earnings view?`,
    };
  }
  if (has("balance", "wallet", "how much salt")) {
    return {
      toolCalls: [tc("chain_read", { query: "wallet.balances" })],
      compose: (c) =>
        `Liquid balance: **${c.liquid.toFixed(2)} SALT** (earned rewards — the 32,000 grant sits staked, not liquid). Staked: **${fmt(c.staked)} SALT**. Address ${c.walletAddr.slice(0, 6)}…${c.walletAddr.slice(-4)}, read from your local node.`,
    };
  }
  if (has("network", "height", "peers", "block", "finality", "status")) {
    return {
      toolCalls: [tc("chain_read", { query: "network.vitals" })],
      compose: (c) =>
        `Chain 40204 is at height **${fmt(c.height)}**, your node sees **${c.peers} peers**, and the last BFT checkpoint settled ${c.finalityAge}s ago (checkpoints land every ~50 blocks). Your node is **${c.nodeState}**.`,
    };
  }
  if (has("journal", "log this", "write this down", "worklog")) {
    const entry = t.replace(/^(please\s+)?(journal|log this|write this down)[:,]?\s*/i, "") || "work note";
    return {
      toolCalls: [tc("journal_append", { entry })],
      compose: (_c, calls) =>
        calls[0].result === "approved"
          ? "Logged to today’s daily note — off-chain, local, and yours. I keep my own worklog there too; open Journal to see the thread."
          : "You declined, so nothing was written. The journal only takes entries you approve.",
    };
  }
  if (has("remember", "note that", "save this", "keep in mind")) {
    const fact = t.replace(/^(please\s+)?(remember|note that|save this|keep in mind)[:,]?\s*/i, "") || "the member’s note";
    return {
      toolCalls: [tc("memory_assert", { fact })],
      compose: (_c, calls) =>
        calls[0].result === "approved"
          ? "Written to your memory graph — you approved the assertion, so it now lives in your personal tenant, witnessed and recallable."
          : "Understood — you declined the write, so nothing was stored. Your memory graph only takes facts you approve.",
    };
  }
  if (has("memory", "recall", "what do you know")) {
    return {
      toolCalls: [tc("memory_recall", { query: t })],
      compose: () =>
        "From your memory graph: your validator key ceremony completed at onboarding, your gateway key is bound to this device, and the chain-facts tenant carries the 40204 contract catalog. Ask me to recall anything specific, or open Storage to walk the constellation.",
    };
  }
  if (has("doc", "how do i", "guide", "tutorial", "learn")) {
    return {
      toolCalls: [tc("docs_link", { topic: t })],
      compose: () =>
        "I linked the closest Atlas guides for your tier in the tutorials rail. The validator operations handbook is the right starting point — it covers sync, heartbeat, and what slashing protection expects from an operator.",
    };
  }
  if (has("go to", "open ", "take me", "navigate")) {
    const route = ["wallet", "node", "storage", "comms", "commissary", "settings"].find((r) => t.includes(r)) || "dashboard";
    return {
      toolCalls: [tc("app_navigate", { route })],
      compose: () => `Done — you're on ${route[0].toUpperCase() + route.slice(1)}.`,
    };
  }
  return {
    toolCalls: [],
    compose: (c) =>
      `I'm your member agent — grounded in your node, wallet, and memory graph. I can read your staking position, break down earnings, recall or (with your approval) write memory, link docs at your tier, and move you around the app. Your node is ${c.nodeState} at height ${fmt(c.height)}. What do you need?`,
  };
}

function tc(name: string, args: Record<string, unknown>): ToolCall {
  return { id: "call_" + Math.random().toString(36).slice(2, 10), name, arguments: JSON.stringify(args) };
}
function tokenize(text: string): string[] {
  return text.split(/(\s+)/).filter(Boolean);
}
function fmt(n: number): string {
  return Math.round(n).toLocaleString("en-US");
}
function wait(ms: number): Promise<void> {
  return new Promise((r) => setTimeout(r, ms));
}
