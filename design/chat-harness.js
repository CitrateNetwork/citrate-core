// =====================================================================
// citrate-core — chat harness
//
// The Dashboard agent speaks to any provider that implements:
//
//   provider.send({ messages, tools, signal, callbacks }) -> Promise<assistantMessage>
//
//   callbacks = {
//     onStatus(status)      'thinking' | 'streaming' | 'tool' | 'done' | 'error'
//     onToken(text)         streamed content delta
//     onToolCall(call)      { id, name, arguments } — host resolves the tool
//                           (HITL gating happens in the host: memory/chain
//                           writes route to the approval queue / Signature
//                           Ceremony) and returns a Promise<resultString>.
//   }
//
// Production wiring (02 §7): swap createDemoProvider for
// createGatewayProvider (inference-gateway local-proxy, OpenAI-compatible,
// cgk_ bearer) with createLocalProvider (llama-server sidecar) as the
// offline fallback. The message/tool shapes below are the OpenAI
// chat-completions shapes, so the swap is transport-only.
// =====================================================================

export const AGENT_SYSTEM_PROMPT = [
  'You are the Citrate member agent inside citrate-core.',
  'You may read the member\u2019s memory graph and local chain state.',
  'Every write (memory_assert, any chain transaction) is proposed, never executed:',
  'writes queue for human approval in the Signature Ceremony.',
  'Speak plainly. Never fabricate numbers; read them through tools.',
].join(' ');

// OpenAI tool schemas — identical shapes ship to the gateway (02 §7).
export const AGENT_TOOLS = [
  {
    type: 'function',
    function: {
      name: 'chain_read',
      description: 'Read local node RPC / contract state (balances, staking position, claimable rewards, network vitals).',
      parameters: {
        type: 'object',
        properties: { query: { type: 'string', enum: ['staking.position', 'wallet.balances', 'earnings.claimable', 'network.vitals'] } },
        required: ['query'],
      },
    },
  },
  {
    type: 'function',
    function: {
      name: 'memory_recall',
      description: 'Recall nodes from the member\u2019s memory graph (local MCP socket).',
      parameters: { type: 'object', properties: { query: { type: 'string' } }, required: ['query'] },
    },
  },
  {
    type: 'function',
    function: {
      name: 'memory_assert',
      description: 'Write a fact to the member\u2019s memory graph. HITL: queues for approval.',
      parameters: { type: 'object', properties: { fact: { type: 'string' } }, required: ['fact'] },
    },
  },
  {
    type: 'function',
    function: {
      name: 'journal_append',
      description: 'Append an entry to the member\u2019s local journal (today\u2019s daily note). Off-chain, HITL: queues for approval.',
      parameters: { type: 'object', properties: { entry: { type: 'string' } }, required: ['entry'] },
    },
  },
  {
    type: 'function',
    function: {
      name: 'docs_link',
      description: 'Link tier-appropriate Atlas documentation.',
      parameters: { type: 'object', properties: { topic: { type: 'string' } }, required: ['topic'] },
    },
  },
  {
    type: 'function',
    function: {
      name: 'app_navigate',
      description: 'Deep-link to a citrate-core page (citrate-core:// scheme).',
      parameters: { type: 'object', properties: { route: { type: 'string' } }, required: ['route'] },
    },
  },
];

// ---------------------------------------------------------------------
// Gateway provider — PRODUCTION PATH (unused by the prototype's demo
// mode, kept wired-shaped so engineering swaps it in directly).
// ---------------------------------------------------------------------
export function createGatewayProvider({ baseUrl, apiKey, model = 'citrate-agent-1' }) {
  return {
    kind: 'gateway',
    label: 'infer.citrate.ai · local-proxy',
    async send({ messages, tools = AGENT_TOOLS, signal, callbacks }) {
      callbacks.onStatus('thinking');
      const res = await fetch(`${baseUrl}/v1/chat/completions`, {
        method: 'POST',
        signal,
        headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${apiKey}` },
        body: JSON.stringify({ model, messages: [{ role: 'system', content: AGENT_SYSTEM_PROMPT }, ...messages], tools, stream: true }),
      });
      if (!res.ok) { callbacks.onStatus('error'); throw new Error(`gateway ${res.status}`); }
      const reader = res.body.getReader();
      const decoder = new TextDecoder();
      let content = '';
      const toolCalls = [];
      callbacks.onStatus('streaming');
      for (;;) {
        const { done, value } = await reader.read();
        if (done) break;
        for (const line of decoder.decode(value, { stream: true }).split('\n')) {
          if (!line.startsWith('data: ') || line.includes('[DONE]')) continue;
          const delta = JSON.parse(line.slice(6)).choices?.[0]?.delta || {};
          if (delta.content) { content += delta.content; callbacks.onToken(delta.content); }
          if (delta.tool_calls) accumulateToolCalls(toolCalls, delta.tool_calls);
        }
      }
      for (const call of toolCalls) {
        callbacks.onStatus('tool');
        await callbacks.onToolCall(call);
      }
      callbacks.onStatus('done');
      return { role: 'assistant', content };
    },
  };
}

function accumulateToolCalls(acc, deltas) {
  for (const d of deltas) {
    acc[d.index] = acc[d.index] || { id: d.id, name: '', arguments: '' };
    if (d.function?.name) acc[d.index].name += d.function.name;
    if (d.function?.arguments) acc[d.index].arguments += d.function.arguments;
  }
}

// Local fallback — llama-server sidecar exposes the same API surface.
export function createLocalProvider({ port = 11544 }) {
  const p = createGatewayProvider({ baseUrl: `http://127.0.0.1:${port}`, apiKey: 'local', model: 'local-gguf' });
  return { ...p, kind: 'local', label: 'running on your machine' };
}

// ---------------------------------------------------------------------
// Demo provider — same contract, scripted reasoning over live sim state.
// getContext() is supplied by the host and returns the current snapshot:
// { height, peers, finalityAge, nodeState, staked, liquid, claimable,
//   earningsToday, walletAddr, memberSince, tier }
// ---------------------------------------------------------------------
export function createDemoProvider(getContext) {
  return {
    kind: 'demo',
    label: 'infer.citrate.ai · local-proxy',
    async send({ messages, callbacks }) {
      const userText = (messages[messages.length - 1]?.content || '').toLowerCase();
      callbacks.onStatus('thinking');
      await wait(900 + Math.random() * 700);

      const plan = routeIntent(userText, getContext);

      for (const call of plan.toolCalls) {
        callbacks.onStatus('tool');
        await wait(350);
        call.result = await callbacks.onToolCall(call);
      }

      const text = plan.compose(getContext(), plan.toolCalls);
      callbacks.onStatus('streaming');
      let out = '';
      for (const token of tokenize(text)) {
        out += token;
        callbacks.onToken(token);
        await wait(14 + Math.random() * 26);
      }
      callbacks.onStatus('done');
      return { role: 'assistant', content: out };
    },
  };
}

function routeIntent(t, getContext) {
  const has = (...ws) => ws.some((w) => t.includes(w));

  if (has('stake', 'staked', 'position', 'validator status')) {
    return {
      toolCalls: [tc('chain_read', { query: 'staking.position' })],
      compose: (c) =>
        `You have **${fmt(c.staked)} SALT** staked in the LiquidStakingPool — your full membership grant, vaulted and attributed to your validator. That meets the 32,000 SALT minimum, so your node ${c.nodeState === 'validating' ? 'is validating and earning' : 'is eligible to validate once running'}. The granted principal stays vaulted until mainnet release; rewards your node earns are yours. Withdrawals of self-added stake carry a 7-day lockup.`,
    };
  }
  if (has('earn', 'reward', 'claim', 'income', 'made today')) {
    return {
      toolCalls: [tc('chain_read', { query: 'earnings.claimable' })],
      compose: (c) =>
        `Today your node has earned **${c.earningsToday.toFixed(2)} SALT** — validation makes up most of it, with smaller pinning and compute shares. Your claimable balance is **${c.claimable.toFixed(2)} SALT**; claims batch until they clear the dust threshold, and claiming signs a claimRewards() transaction through the ceremony. Want me to take you to the earnings view?`,
    };
  }
  if (has('balance', 'wallet', 'how much salt')) {
    return {
      toolCalls: [tc('chain_read', { query: 'wallet.balances' })],
      compose: (c) =>
        `Liquid balance: **${c.liquid.toFixed(2)} SALT** (earned rewards — the 32,000 grant sits staked, not liquid). Staked: **${fmt(c.staked)} SALT**. Address ${c.walletAddr.slice(0, 6)}…${c.walletAddr.slice(-4)}, read from your local node.`,
    };
  }
  if (has('network', 'height', 'peers', 'block', 'finality', 'status')) {
    return {
      toolCalls: [tc('chain_read', { query: 'network.vitals' })],
      compose: (c) =>
        `Chain 40204 is at height **${fmt(c.height)}**, your node sees **${c.peers} peers**, and the last BFT checkpoint settled ${c.finalityAge}s ago (checkpoints land every ~50 blocks). Your node is **${c.nodeState}**.`,
    };
  }
  if (has('journal', 'log this', 'write this down', 'worklog')) {
    const entry = t.replace(/^(please\s+)?(journal|log this|write this down)[:,]?\s*/i, '') || 'work note';
    return {
      toolCalls: [tc('journal_append', { entry })],
      compose: (_c, calls) =>
        calls[0].result === 'approved'
          ? 'Logged to today\u2019s daily note — off-chain, local, and yours. I keep my own worklog there too; open Journal to see the thread.'
          : 'You declined, so nothing was written. The journal only takes entries you approve.',
    };
  }
  if (has('remember', 'note that', 'save this', 'keep in mind')) {
    const fact = t.replace(/^(please\s+)?(remember|note that|save this|keep in mind)[:,]?\s*/i, '') || 'the member\u2019s note';
    return {
      toolCalls: [tc('memory_assert', { fact })],
      compose: (_c, calls) =>
        calls[0].result === 'approved'
          ? 'Written to your memory graph — you approved the assertion, so it now lives in your personal tenant, witnessed and recallable.'
          : 'Understood — you declined the write, so nothing was stored. Your memory graph only takes facts you approve.',
    };
  }
  if (has('memory', 'recall', 'what do you know')) {
    return {
      toolCalls: [tc('memory_recall', { query: t })],
      compose: () =>
        'From your memory graph: your validator key ceremony completed at onboarding, your gateway key is bound to this device, and the chain-facts tenant carries the 40204 contract catalog. Ask me to recall anything specific, or open Storage to walk the constellation.',
    };
  }
  if (has('doc', 'how do i', 'guide', 'tutorial', 'learn')) {
    return {
      toolCalls: [tc('docs_link', { topic: t })],
      compose: () =>
        'I linked the closest Atlas guides for your tier in the tutorials rail. The validator operations handbook is the right starting point — it covers sync, heartbeat, and what slashing protection expects from an operator.',
    };
  }
  if (has('go to', 'open ', 'take me', 'navigate')) {
    const route = ['wallet', 'node', 'storage', 'comms', 'commissary', 'settings'].find((r) => t.includes(r)) || 'dashboard';
    return {
      toolCalls: [tc('app_navigate', { route })],
      compose: () => `Done — you're on ${route[0].toUpperCase() + route.slice(1)}.`,
    };
  }
  return {
    toolCalls: [],
    compose: (c) =>
      `I'm your member agent — grounded in your node, wallet, and memory graph. I can read your staking position, break down earnings, recall or (with your approval) write memory, link docs at your tier, and move you around the app. Your node is ${c.nodeState} at height ${fmt(c.height)}. What do you need?`,
  };
}

function tc(name, args) {
  return { id: 'call_' + Math.random().toString(36).slice(2, 10), name, arguments: JSON.stringify(args) };
}
function tokenize(text) {
  return text.split(/(\s+)/).filter(Boolean);
}
function fmt(n) {
  return Math.round(n).toLocaleString('en-US');
}
function wait(ms) {
  return new Promise((r) => setTimeout(r, ms));
}
