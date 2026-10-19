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
//                           (HIC gating happens in the host: memory/chain
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
        properties: { query: { type: 'string', enum: ['membership.bond', 'wallet.balance', 'validator.rewards', 'network.fixture'] } },
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
      description: 'Write a fact to the member\u2019s memory graph. HIC: queues for approval.',
      parameters: { type: 'object', properties: { fact: { type: 'string' } }, required: ['fact'] },
    },
  },
  {
    type: 'function',
    function: {
      name: 'journal_append',
      description: 'Append an entry to the member\u2019s local journal (today\u2019s daily note). Off-chain, HIC: queues for approval.',
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
// { fx (C1 fixture), nodeState, hasGrant, routing, webSearch, via, walletAddr, tier }
// ---------------------------------------------------------------------
export function createDemoProvider(getContext) {
  return {
    kind: 'demo',
    label: 'demo replies (contract-safe)',
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

// Slice 1 (0.5.2): every reply follows the chain team's UI truth contract v1. Chain numbers come
// only from the C1 fixture (labelled as such), finality is never described as running, and there
// are no earnings projections, investment words or SALT prices. Unknown is said as unknown.
function routeIntent(t, getContext) {
  const has = (...ws) => ws.some((w) => t.includes(w));
  const fx = () => getContext().fx;
  const viaNote = (c) => (c.via === 'gateway' ? '(Made by infer.citrate.ai for this one reply, because you allowed it.)\n' : c.routing === 'local' || c.via === 'local' ? '(Made by the local model on this machine.)\n' : '');

  if (has('search the web', 'look up', 'search for', 'web search', 'google')) {
    const ctx = getContext();
    if (!ctx.webSearch) {
      return { toolCalls: [], compose: () => 'Web search is off, so I can’t look that up. You can turn it on in Hermes › Controls › Web search. Until then I answer from this machine only.' };
    }
    return {
      toolCalls: [tc('web_search', { query: t })],
      compose: () => 'I searched through your local SearXNG and read three pages on this machine. (Prototype reply: the real one cites each page it read.)\nGhostDAG is the ordering rule a BlockDAG uses: it picks a well-connected set of blocks and orders the whole DAG from that set, so blocks made at the same time are kept instead of thrown away.',
    };
  }
  if (has('plan', 'draft', 'research', 'rewrite')) {
    return {
      toolCalls: [tc('memory_recall', { query: 'journal: this week' })],
      compose: (c) => viaNote(c) + 'From your journal this week: 1) finish the slice 1 review, 2) read the UI truth contract before slice 2, 3) leave Inbox triage running. Want me to save this as a journal page? It will wait for your approval first.',
    };
  }
  if (has('stake', 'staked', 'bond', 'grant', 'position', 'validator status')) {
    // The one grant explanation (core src/data/grantCopy.ts, PR #349; contract §5.4, §5.6, §5.7).
    const DISCLAIMER = 'The grant is not purchased, and it is not an investment.';
    const BETANET = 'Betanet. What carries over to mainnet is set in the genesis spec.';
    return {
      toolCalls: [tc('chain_read', { query: 'membership.bond' })],
      compose: (c) => c.hasGrant
        ? 'Your membership comes with a 32,000 SALT grant, locked in your bond for about a year. It is not staked until you activate the bond. Its state reads “Bond locked · not staked” (MemberBond.activated()). The lock end comes from MemberBond.unlockTimestamp() and unlockBlock(); this prototype has no read of them, so I can’t tell you the date. ' + DISCLAIMER + ' ' + BETANET
        : 'You are not a member yet, so there is no bond. A membership comes with a 32,000 SALT grant, locked in a bond. ' + DISCLAIMER,
    };
  }
  if (has('earn', 'reward', 'claim', 'income', 'made today')) {
    // Contract §1 Rewards: no "today", no estimates, no projections. Fixture numbers are labelled inline (ruling 3).
    return {
      toolCalls: [tc('chain_read', { query: 'validator.rewards' })],
      compose: (c) => 'Fixture (40204 block ' + fmt(c.fx.block) + ', not live): ' + c.fx.activeValidators + ' validators are active. Chain team, sampled 2026-10-19: 1 of them produces blocks, and only a validator that produces blocks receives validator rewards. Your rewards come only from ValidatorRegistry.rewardsOf for your validator key. This prototype has no read of it, so your rewards are unknown, not zero. No projections.',
    };
  }
  if (has('balance', 'wallet', 'how much salt')) {
    return {
      toolCalls: [tc('chain_read', { query: 'wallet.balance' })],
      compose: (c) => 'Your balance comes from eth_getBalance on “latest” for ' + c.walletAddr.slice(0, 6) + '…' + c.walletAddr.slice(-4) + '. This prototype has no live read, so it is unknown (—), not zero. Wallet is redrawn in slice 2.',
    };
  }
  if (has('network', 'height', 'peers', 'block', 'finality', 'status')) {
    return {
      toolCalls: [tc('chain_read', { query: 'network.fixture' })],
      compose: (c) => {
        const f = c.fx;
        return 'Fixture (40204 block ' + fmt(f.block) + ', ' + f.capturedAt + ', not live): Citrate betanet, chain 40204, blocks every ~' + f.blockTime + ' s, ' + f.activeValidators + ' active validators. Chain team, sampled 2026-10-19: 1 validator produces blocks. Checkpoint finality is not running yet, so I count confirmations, not finality. Your node: ' + c.nodeState + '.';
      },
    };
  }
  if (has('journal', 'log this', 'write this down', 'worklog')) {
    const entry = t.replace(/^(please\s+)?(journal|log this|write this down)[:,]?\s*/i, '') || 'work note';
    return {
      toolCalls: [tc('journal_append', { entry })],
      compose: (_c, calls) =>
        calls[0].result === 'approved'
          ? 'Added to today’s page. It is local and yours.'
          : 'You declined, so nothing was written. The journal only takes entries you approve.',
    };
  }
  if (has('remember', 'note that', 'save this', 'keep in mind')) {
    const fact = t.replace(/^(please\s+)?(remember|note that|save this|keep in mind)[:,]?\s*/i, '') || 'the member’s note';
    return {
      toolCalls: [tc('memory_assert', { fact })],
      compose: (_c, calls) =>
        calls[0].result === 'approved'
          ? 'Written to your memory graph, because you approved it.'
          : 'You declined, so nothing was stored. Your memory graph only takes facts you approve.',
    };
  }
  if (has('memory', 'recall', 'what do you know')) {
    return {
      toolCalls: [tc('memory_recall', { query: t })],
      compose: () => 'From your memory graph: you prefer short replies (contradicted once; resolve it in Hermes › Learn), and your shared folder is ~/projects/site. Ask me to recall anything specific.',
    };
  }
  if (has('doc', 'how do i', 'guide', 'tutorial')) {
    return {
      toolCalls: [tc('docs_link', { topic: t })],
      compose: () => 'The Atlas guides for your tier cover running a node, daemons and approvals. Start with “How approvals work”.',
    };
  }
  if (has('go to', 'open ', 'take me', 'navigate')) {
    const route = ['groups', 'wallet', 'node', 'store', 'settings'].find((r) => t.includes(r)) || 'hermes';
    return {
      toolCalls: [tc('app_navigate', { route })],
      compose: () => `Done. You're on ${route[0].toUpperCase() + route.slice(1)}.`,
    };
  }
  return {
    toolCalls: [],
    compose: (c) => viaNote(c) + 'I’m Hermes, running on this machine. I can read your journal and memory, search the web if you turn it on, and propose changes that wait for your review. Ask me about the network and I’ll say where each number comes from.',
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
