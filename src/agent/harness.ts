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
// Rule 1: the label must not claim a gateway/local-proxy it never calls — this
// is a BUILT-IN demo agent (scripted), and it says so.
// ---------------------------------------------------------------------
export function createDemoProvider(getContext: () => AgentContext): ChatProvider {
  return {
    kind: "demo",
    label: "built-in demo agent",
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

// ---------------------------------------------------------------------
// Real provider — CORE-AI1 (@rule8). Calls the Rust `ai_chat` command (via the
// injected `infer` fn, so this module keeps no bridge import → no store cycle),
// which reads the SEALED {baseURL, model, apiKey} for `providerId` and POSTs the
// OpenAI /v1/chat/completions body from Rust. The key NEVER touches the webview
// and the webview NEVER supplies the URL (exfil-binding). This WP is plain chat +
// live-context injection with NO tool loop; the REAL completion is revealed via a
// token animation (honest — it is the real content, just streamed for display).
// ---------------------------------------------------------------------
export type InferFn = (providerId: string, messagesJson: string, contextJson: string) => Promise<string>;

export function createRealProvider(providerId: string, getContext: () => AgentContext, infer: InferFn): ChatProvider {
  return {
    kind: "real",
    label: "provider · " + providerId,
    async send({ messages, callbacks }) {
      callbacks.onStatus("thinking");
      // The live app context is passed to Rust as an opaque JSON snapshot; Rust
      // injects it as a system line so the model grounds its numbers (Rule 1).
      const contextJson = JSON.stringify(getContext());
      const messagesJson = JSON.stringify(messages.map((m) => ({ role: m.role, content: m.content })));
      let content: string;
      try {
        content = await infer(providerId, messagesJson, contextJson);
      } catch (e) {
        callbacks.onStatus("error");
        throw e;
      }
      // Reveal the REAL completion with a display-only token animation. There is no
      // tool loop this WP and the content is never fabricated — it is exactly what
      // the provider returned, streamed for the same UX as the demo agent.
      callbacks.onStatus("streaming");
      let out = "";
      for (const token of tokenize(content)) {
        out += token;
        callbacks.onToken(token);
        await wait(8 + Math.random() * 18);
      }
      callbacks.onStatus("done");
      return { role: "assistant", content: out };
    },
  };
}

// ---------------------------------------------------------------------
// Local provider — BC-3.2. Calls the Rust `ai_chat_local` command (via the
// injected `inferLocal` fn) which POSTs to the bundled `llama-server` on the
// Rust-owned loopback endpoint — NO api key, NO caller-supplied URL. This is
// only ever selected when the inference state is `ready` (model verified + server
// healthy); otherwise the store routes to gateway/demo, so the local reply is
// never fabricated (Rule 1). Plain chat + live-context injection, no tool loop;
// the REAL completion is revealed via a display-only token animation.
// ---------------------------------------------------------------------
export type InferLocalFn = (messagesJson: string, contextJson: string) => Promise<string>;

export function createLocalProvider(getContext: () => AgentContext, inferLocal: InferLocalFn): ChatProvider {
  return {
    kind: "local",
    label: "local model · llama-server",
    async send({ messages, callbacks }) {
      callbacks.onStatus("thinking");
      const contextJson = JSON.stringify(getContext());
      const messagesJson = JSON.stringify(messages.map((m) => ({ role: m.role, content: m.content })));
      let content: string;
      try {
        content = await inferLocal(messagesJson, contextJson);
      } catch (e) {
        callbacks.onStatus("error");
        throw e;
      }
      // Reveal the REAL local completion with a display-only token animation. No
      // tool loop this WP; the content is exactly what the local model returned.
      callbacks.onStatus("streaming");
      let out = "";
      for (const token of tokenize(content)) {
        out += token;
        callbacks.onToken(token);
        await wait(8 + Math.random() * 18);
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
        // Rule 1: the demo agent can't reach the memory daemon, so an approval
        // does NOT durably store anything — don't claim it "now lives" in the
        // graph. The real write path lands when mem-mcp is bundled + running.
        calls[0].result === "approved"
          ? "You approved the write in the ceremony — but the demo agent isn't connected to your memory daemon, so nothing was durably stored. When the memory daemon is running and chat uses a real provider, an approved assertion will actually land in your personal tenant."
          : "You declined, so nothing was written. (In demo mode the write wouldn't persist anyway — the demo agent isn't connected to your memory daemon.)",
    };
  }
  if (has("memory", "recall", "what do you know")) {
    return {
      // Rule 1: the demo agent has no connection to your memory daemon, so it
      // must NOT invent recalled facts. It says so honestly and points at the
      // real surface. (Real recall arrives when the mem-mcp daemon is bundled +
      // running and chat routes to a real provider with the memory tool loop.)
      toolCalls: [],
      compose: () =>
        "I can't read your memory graph in demo mode — the demo agent isn't connected to your memory daemon, so I won't invent what it holds. Open Storage to walk the real constellation, and configure a provider (or run the local model) to let me recall from it directly.",
    };
  }
  if (has("doc", "how do i", "guide", "tutorial", "learn")) {
    return {
      // Rule 1: "docs_link" was a no-op that falsely claimed "I linked the closest
      // guides." The demo agent links nothing — say so honestly.
      toolCalls: [],
      compose: () =>
        "Docs linking isn't available in demo mode — I can't add anything to the tutorials rail from here. The Atlas guides open from the tutorials rail on the Dashboard; the validator operations handbook is a good starting point for sync, heartbeat, and slashing protection.",
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
