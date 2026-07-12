// =====================================================================
// citrate-core — seed data
// In production these arrive from named sources (Rule 11): the signed
// Commissary catalog manifest (core-membership), Atlas, the comms
// notifications API, and the local memory MCP socket. Shapes match the
// spec so wiring replaces this module, not the UI.
// =====================================================================

export const CONTRACTS = {
  liquidStakingPool: { name: 'LiquidStakingPool', addr: '0xfd27a3c9d14be081f7550dbb6ab5b3c2891e685e' },
  entryPoint:        { name: 'EntryPoint v0.7',   addr: '0x077Fdc05e4c17ee0e9c2d6b3f28a67c92be954Ef' },
  paymaster:         { name: 'CitratePaymaster',  addr: '0x884c11b7de08e2a4b91f9c33da4c8b0a63f60d28' },
  memberSbt:         { name: 'CitrateMemberSBT',  addr: '0x9a3f6e21c88db04d7a15e9b02f764a80413cc771' },
  stakeVault:        { name: 'MembershipStakeVault', addr: '0x5b21d90cf3a6488ba7e01c2d94f7e3ab1042e90a' },
};

export const PERSONAS = {
  p1: { id: 'p1', name: 'Dana Okafor', initials: 'DO', label: 'P1 · The Joiner', tier: 'pilot', tierLabel: 'Pilot member', role: 'member', org: null, fresh: true,
        blurb: 'Fresh install — walks S0–S6.' },
  p2: { id: 'p2', name: 'Marcus Bell', initials: 'MB', label: 'P2 · The Operator', tier: 'pilot', tierLabel: 'Pilot member', role: 'operator', org: null, fresh: false,
        blurb: 'Onboarded, node validating, deep telemetry.' },
  p3: { id: 'p3', name: 'Priya Anand', initials: 'PA', label: 'P3 · The Builder', tier: 'pilot', tierLabel: 'Pilot member', role: 'builder', org: null, fresh: false,
        blurb: 'SDKs, gateway key, agents on the MCP socket.' },
  p4: { id: 'p4', name: 'R. Calloway', initials: 'RC', label: 'P4 · The Org Seat', tier: 'enterprise', tierLabel: 'Enterprise · BA-7', role: 'org-seat', org: 'BA-7', fresh: false,
        blurb: 'Boeing-class seat — org-scoped doors render.' },
};

// tier order for gating math
export const TIERS = ['free', 'pilot', 'enterprise'];

export const CATALOG = {
  apps: [
    { id: 'citrate-native', name: 'Citrate Native', kind: 'app', desc: 'Light node and wallet. The pocket identity — citrate-core is its full-node counterpart.',
      version: '0.9.2-beta.2', size: '38.4 MB', status: 'Beta', minTier: 'free',
      platforms: ['macOS arm64', 'macOS x86_64', 'Linux x86_64'],
      checksum: 'sha256:2f8e17aa…c9c41' },
    { id: 'citrate-studio', name: 'Citrate Studio', kind: 'app', desc: 'Agent authoring and evaluation workbench for partner teams.',
      version: '1.0.0-rc.3', size: '112.7 MB', status: 'Release Candidate', minTier: 'pilot',
      platforms: ['macOS arm64', 'Linux x86_64'],
      checksum: 'sha256:9b02d4f1…7e2a8' },
    { id: 'nist-agent', name: 'NIST Agent', kind: 'app', desc: 'Compliance-mapping agent. NIST 800-53 crosswalks over your attestation lineage.',
      version: '0.2.1-alpha', size: '54.1 MB', status: 'Pre-alpha', minTier: 'pilot',
      platforms: ['Linux x86_64'],
      checksum: 'sha256:71c3e8b0…d4f19' },
    { id: 'lattice-observatory', name: 'Lattice Observatory', kind: 'micro-app', desc: 'GhostDAG topology panel. Opens in an isolated window with a capability-scoped bridge.',
      version: '0.4.0', size: 'web surface', status: 'Beta', minTier: 'pilot',
      platforms: ['isolated webview'],
      capabilities: ['identity token hand-off', 'EIP-1193 provider (read + propose)'],
      checksum: 'manifest-signed' },
    { id: 'ba-supplier-pack', name: 'Supplier Attest Pack', kind: 'app', desc: 'Org-scoped attestation workflows for the BA-7 supplier network.',
      version: '2.3.0', size: '61.9 MB', status: 'GA', minTier: 'enterprise', orgScope: 'BA-7',
      platforms: ['macOS arm64', 'Linux x86_64'],
      checksum: 'sha256:e33a90cd…1b7f2' },
  ],
  sdks: [
    { id: 'citrate-js', name: 'citrate-js', registry: 'npm', install: 'npm install @citrate/sdk', desc: 'TypeScript SDK — chain reads, UserOps, memory client.', minTier: 'free', docs: 'atlas/sdk/js' },
    { id: 'citrate-ai-sdk', name: 'citrate-ai-sdk', registry: 'PyPI', install: 'pip install citrate-ai', desc: 'Python SDK — gateway inference, MCP tools, agent harness.', minTier: 'free', docs: 'atlas/sdk/python' },
    { id: 'marketplace-sdk', name: 'marketplace-sdk', registry: 'GitHub Packages', install: 'npm install @citrate/marketplace --registry=ghcr', desc: 'x402 marketplace bidding and settlement. Registry auth required.', minTier: 'pilot', docs: 'atlas/sdk/marketplace' },
  ],
  docs: [
    { id: 'd1', name: 'Validator operations handbook', tier: 'member', minTier: 'pilot', desc: 'Sync, heartbeat, slashing protection, claim batching.' },
    { id: 'd2', name: 'Network primer — GhostDAG on chain 40204', tier: 'public', minTier: 'free', desc: 'Consensus, checkpoints, finality for operators.' },
    { id: 'd3', name: 'Memory graph — agent integration guide', tier: 'member', minTier: 'pilot', desc: 'MCP socket, capability grants, assert/recall semantics.' },
    { id: 'd4', name: 'LVM precompile reference', tier: 'commercial', minTier: 'pilot', desc: 'The 10 precompiles: 7 AI, 3 x402.' },
    { id: 'd5', name: 'Confidential — federation deployment annex', tier: 'confidential', minTier: 'enterprise', desc: 'On-prem topology patterns for primes.' },
    { id: 'd6', name: 'BA-7 integration runbook', tier: 'org', minTier: 'enterprise', orgScope: 'BA-7', desc: 'Org-scoped: supplier node enrollment.' },
  ],
  services: [
    { id: 'citratescan', name: 'CitrateScan', desc: 'The explorer — the federation\u2019s quality bar.', url: 'scan.citrate.ai' },
    { id: 'dashboard-web', name: 'Network dashboard', desc: 'Public network vitals.', url: 'dashboard.citrate.ai' },
    { id: 'buyer-webapp', name: 'Marketplace', desc: 'x402 buyer surface — trading stays here, not in-app.', url: 'market.citrate.ai' },
    { id: 'memrizz', name: 'Memrizz', desc: 'Memory constellation, full canvas.', url: 'memrizz.citrate.ai' },
    { id: 'comms-web', name: 'Comms', desc: 'Rooms and relays.', url: 'comms.citrate.ai' },
    { id: 'dataroom', name: 'Dataroom', desc: 'Filings and diligence.', url: 'dataroom.citrate.ai' },
  ],
};

export const TUTORIALS = [
  { id: 't1', title: 'Run your first validator', minutes: 8, minTier: 'free', path: 'atlas/tutorials/first-validator' },
  { id: 't2', title: 'Point Claude at your memory graph', minutes: 5, minTier: 'pilot', path: 'atlas/tutorials/mcp-claude' },
  { id: 't3', title: 'How claims batch — earnings mechanics', minutes: 4, minTier: 'pilot', path: 'atlas/tutorials/claims' },
  { id: 't4', title: 'x402 marketplace primer', minutes: 12, minTier: 'pilot', path: 'atlas/tutorials/x402' },
];

export const PINGS = [
  { id: 'g1', actor: 'annika.v', room: 'operators / mainnet-prep', kind: 'mention', ago: '6m',
    note: 'mentioned you' },
  { id: 'g2', actor: 'coop-announce', room: 'governance', kind: 'announcement', ago: '2h',
    note: 'BR1J amendment window opened' },
  { id: 'g3', actor: 'jt', room: 'builders / mcp', kind: 'reply', ago: '5h',
    note: 'replied to your thread' },
];

export const NODE_LOG_TEMPLATES = [
  'ghostdag: accepted block {h} · blue score {h} · anticone 3/18',
  'net: peer handshake ok · {peers} active',
  'exec: lvm batch applied · 14 txs · 0 reverts',
  'finality: checkpoint vote cast · committee round {r}',
  'heartbeat: node-agent ack · 127.0.0.1:19600 · 84ms',
  'storage: rocksdb compaction L1 · 213 MB',
  'ecvrf: election lost round {r} · next eligibility in 4 blocks',
  'ghostdag: merged 2 parallel parents at {h}',
  'pin: challenge window open · cid bafy…kq4e',
  'mempool: 41 pending · 3 priority',
];

// Constellation — per-user memory instance. tenant: personal | chain-facts
export const GRAPH = {
  nodes: [
    { id: 'n1', label: 'validator key ceremony', tenant: 'personal', kind: 'event', x: 320, y: 210, z: 1.25, detail: 'Keystore created at onboarding S4. OS keyring: secured. Never leaves this machine.' },
    { id: 'n2', label: 'membership SBT #4187', tenant: 'personal', kind: 'credential', x: 470, y: 150, z: 1.1, detail: 'CitrateMemberSBT minted at S5. Non-transferable. Bound to sub-hash.' },
    { id: 'n3', label: 'gateway key · cgk_…7f2a', tenant: 'personal', kind: 'credential', x: 610, y: 240, z: 0.9, detail: 'Issued from Settings. Stored in OS keyring; revocable server-side.' },
    { id: 'n4', label: 'prefers reduced telemetry', tenant: 'personal', kind: 'preference', x: 250, y: 330, z: 0.8, detail: 'Asserted via chat 2026-07-08. Approved by you.' },
    { id: 'n5', label: 'node data dir — ~/.citrate/core', tenant: 'personal', kind: 'fact', x: 420, y: 360, z: 1.0, detail: 'Encrypted at rest. Master key in OS keyring.' },
    { id: 'n6', label: 'agent: research-runner', tenant: 'personal', kind: 'agent', x: 560, y: 400, z: 1.15, detail: 'External agent connected over the MCP socket. Write scope: your grant only.' },
    { id: 'c1', label: 'chain 40204 params', tenant: 'chain-facts', kind: 'fact', x: 760, y: 180, z: 1.2, detail: 'GhostDAG k=18 · BFT committee 100 · checkpoint every 50 blocks (~25s).' },
    { id: 'c2', label: 'LiquidStakingPool', tenant: 'chain-facts', kind: 'contract', x: 880, y: 260, z: 1.0, detail: '0xfd27…685e · min validator stake 32,000 SALT · 7-day unstake lockup.' },
    { id: 'c3', label: 'CitratePaymaster', tenant: 'chain-facts', kind: 'contract', x: 800, y: 360, z: 0.85, detail: '0x884c…d28 · category budgets: first-op, standard daily, recovery.' },
    { id: 'c4', label: 'EntryPoint v0.7', tenant: 'chain-facts', kind: 'contract', x: 930, y: 150, z: 0.9, detail: '0x077F…54Ef · ERC-4337 UserOp entry.' },
    { id: 'c5', label: 'your grant + stake · S5', tenant: 'chain-facts', kind: 'event', x: 700, y: 300, z: 1.3, detail: '32,000 SALT → MembershipStakeVault → staked. Vaulted until mainnet release.' },
    { id: 'c6', label: 'MembershipStakeVault', tenant: 'chain-facts', kind: 'contract', x: 980, y: 330, z: 1.05, detail: '0x5b21…e90a · principal locked for membership term; rewards flow to you.' },
    { id: 'c7', label: 'enhanced rewards pools', tenant: 'chain-facts', kind: 'fact', x: 860, y: 430, z: 0.8, detail: 'performance 30% · AI 25% · network-health 20% · staking 25%.' },
    { id: 'n7', label: 'first claim — 12.41 SALT', tenant: 'personal', kind: 'event', x: 500, y: 280, z: 0.95, detail: 'claimRewards() signed 2026-07-09. Tx witnessed on 40204.' },
  ],
  links: [
    ['n1', 'n2'], ['n2', 'c5'], ['c5', 'c2'], ['c5', 'c6'], ['c2', 'c1'], ['c3', 'c4'],
    ['c1', 'c4'], ['n3', 'n6'], ['n1', 'n5'], ['n7', 'c2'], ['n7', 'n2'], ['c6', 'c7'],
    ['n4', 'n1'], ['n6', 'n5'], ['c2', 'c6'],
  ],
};

export const COACH_STEPS = [
  { id: 'earn', title: 'Your node earns while this window is closed', body: 'The vitals strip is chain truth from your local node — height, peers, finality, and today\u2019s earnings, decomposed honestly on the Node page.' },
  { id: 'chat', title: 'The agent is grounded in your machine', body: 'It reads your memory graph and local chain state. Anything it wants to write comes back to you for approval — every time.' },
  { id: 'commissary', title: 'The Commissary is your tier\u2019s store-room', body: 'Apps, SDKs, docs, and services. Locked cards name the one action that unlocks them. Downloads are signed and checksum-verified.' },
];

export function makeAddr(seed: string) {
  let h = 2166136261;
  for (const ch of seed) { h = Math.imul(h ^ ch.charCodeAt(0), 16777619); }
  const hex = () => { h = Math.imul(h ^ (h >>> 13), 0x5bd1e995); return (h >>> 0).toString(16).padStart(8, '0'); };
  return '0x' + (hex() + hex() + hex() + hex() + hex()).slice(0, 40);
}

export function makeHash() {
  const c = '0123456789abcdef';
  let s = '0x';
  for (let i = 0; i < 64; i++) s += c[(Math.random() * 16) | 0];
  return s;
}
