---
created: 2026-09-30T00:00:00Z
branch: release/0.5.0-hermes-upskill
author: Larry Klosowski + Claude Opus 5.5
status: planset (Stage-2, red-teamed 2026-09-30)
planset: 2026-09-30-hermes-upskill
code: HUP
repo: citrate-core
companions: 00_OVERVIEW.md, 02_ARCHITECTURE.md
note: Web research 2026-09-29/30. Reddit/X complaints came via secondary roundups (Reddit fetch blocked). Star counts approximate.
---

# External Research

## 1. Nous Research hermes-agent: design lessons

The upstream agent (https://github.com/NousResearch/hermes-agent, MIT, v0.21.x) has
agentskills.io skills with a self-writing "learning loop", layered memory, 60+ tools in
toolsets, an MCP client, `delegate_task` sub-agents, a natural-language cron, and 7
execution backends. It requires a ≥64k context.

**Recurring complaints and our answer:**

| # | Complaint | Evidence | HUP answer |
|---|---|---|---|
| 1 | Self-evaluation always "success"; failures get baked into skills/memory | https://kilo.ai/openclaw/vs-hermes ; https://aiagentstore.ai/agentic-ai-and-workflow-automation/en/the-20-biggest-problems-with-hermes-agent-what-thousands-of-reddit-and-x-users-are-actually-struggling-with-ranked | D-22, US-1.3: verifiers decide; persist = verified + accepted |
| 2 | Self-improvement overwrites hand-tuned skills | https://docs.bswen.com/blog/2026-04-07-hermes-ai-overwrites-skills/ | Proposals only; user accept; skill provenance + hashes |
| 3 | Context bloat (~16.8k tokens for "hello"; tool schemas dominate) | https://aguyintech.com/why-hermes-agent-is-eating-your-context-window/ ; issues #6839, #13332, #23767 | Tool retrieval top-K; tokenizer-true budget; compaction must shrink |
| 4 | Hangs/stalls (400+ issues) | issue #85125 | Deadlines on every tool, Stop liveness (TLA+), async commands |
| 5 | Fragile tool calls on small local models; raw JSON in chat | issues #104603, #29677, #104396 | `--jinja` native templates, lazy grammar, per-tier eval gate |
| 6 | Setup friction | issues #87093, #2825 | One signed installer, everything bundled (D-21) |
| 7 | Fast releases break things | #63047, #83683, #122222, #78647 | Gates + red-green + eval CI |
| 8 | Unsafe defaults (unrestricted shell, approvals off, skill supply chain) | https://labs.cloudsecurityalliance.org/research/csa-research-note-hermes-agent-cves-20260504-csa-styled/ ; https://thehackernews.com/2026/07/hacker-runs-hermes-ai-agent-unattended.html ; https://arxiv.org/pdf/2605.11418 | HIC levels, grants, allowlists, signed capsules, skill review |
| 9 | Hidden egress / cost surprises | #45058, #110912 | Egress opt-in + labelled; spend budgets visible |
| 10 | Can't see what it's doing | roundups above | Activity monitor, "why am I waiting", event stream, pop-outs |

## 2. Making small models reliable agents

- llama.cpp function calling with `--jinja` + lazy grammars:
  https://github.com/ggml-org/llama.cpp/blob/master/docs/function-calling.md
- Tool search / deferred loading (85% fewer tokens; accuracy 49→74%):
  https://anthropic.com/engineering/advanced-tool-use
- Skills as progressive disclosure: https://agentskills.io/home ,
  https://github.com/agentskills/agentskills
- Local model tool-use benchmarks via Hermes Agent (Qwen 3.6 35B-A3B 91.0, 27B 89,
  Gemma 4 26B 81.4): https://github.com/MiaAI-Lab/Best-Local-Model_Agentic-Workflows_2026
- Fine-tuning for tool use: Hammer https://arxiv.org/pdf/2410.04587 ; xLAM
  https://huggingface.co/learn/cookbook/en/function_calling_fine_tuning_llms_on_xlam ;
  SFT vs RL https://arxiv.org/html/2609.17848
- Self-verifying loops:
  https://www.pulumi.com/blog/self-verifying-ai-agents-vercels-agent-browser-in-the-ralph-wiggum-loop/

## 3. MCP (spec 2026-07-28)

Stateless requests with `_meta`; `Mcp-Method`/`Mcp-Name` headers; `input_required`
round trips; `ttlMs`; Tasks as an extension; CIMD over dynamic client registration;
**Roots, Sampling, Logging, HTTP+SSE deprecated**. Tool annotations are hints, not
enforcement.
https://blog.modelcontextprotocol.io/posts/2026-07-28/ ,
https://blog.modelcontextprotocol.io/posts/2026-03-16-tool-annotations/ ,
https://blog.modelcontextprotocol.io/posts/2025-11-25-first-mcp-anniversary/

## 4. Browser and search

- Chrome DevTools MCP (`--autoConnect`): https://github.com/ChromeDevTools/chrome-devtools-mcp
- Playwright MCP (+ extension bridge): https://github.com/microsoft/playwright-mcp
- agent-browser (ref snapshots, ~200–400 tok/page): https://github.com/vercel-labs/agent-browser
- Stagehand: https://github.com/browserbase/stagehand ; dev-browser skill: https://github.com/SawyerHood/dev-browser
- SearXNG: https://github.com/searxng/searxng ; Jina Reader: https://github.com/jina-ai/reader

## 5. System-1 models: TypeSafe Jev (D-14)

TypeSafe AI's **Jev** (released 2026-09-15) returns typed, probability-weighted
decisions over a fixed option set instead of free text. It is priced per input token
with free output.
- Official skill: https://github.com/typesafe-ai/skills (MIT; SKILL.md; plugin install).
- Browser/computer-use scaffolding: https://github.com/ThinkFlowLab/system1-agents
  (Apache-2.0). Its self-reported single WebVoyager task was ~4× faster and ~16× cheaper
  than a chat model; unverified.
- Explainers: https://www.firecrawl.dev/blog/what-is-jev ,
  https://www.langchain.com/blog/building-a-harness-with-jev

It is a hosted API (egress + key), so it is an opt-in backend behind our local `decide()`
slot.

## 6. Solidity agent tooling

- Slither MCP: https://github.com/trailofbits/slither-mcp
- Aderyn + Slither MCP: https://github.com/mariano-aguero/solidity-audit-mcp
- Medusa (AGENTS.md for coding agents): https://github.com/crytic/medusa/blob/master/AGENTS.md
- LLM-generated properties: https://arxiv.org/pdf/2607.23308
- OpenZeppelin Contracts MCP / Wizard: https://mcp.openzeppelin.com/
- Solady: https://github.com/Vectorized/solady

## 7. Agent UX patterns

Step timelines with a live task list; auto-approve with consequential-action checks;
live artifacts; pop-out browsers; visible, reversible actions.
https://www.setproduct.com/blog/ai-agent-ui-design-patterns ,
https://claude.com/blog/cowork-chrome-side-panel ,
https://makeyouragent.ai/blog/agent-ux-patterns-visible-reversible-workflows
