# Agents

An agent on Citrate is a member like any other — it has an identity, it acts under a named
human, and it is governed by the HIC model. What makes agents safe to run is that they are
**keyless**: an agent never holds a signing key and can never sign on its own.

The desktop app can run an agent harness (Hermes) as a supervised local sidecar. The agent
can do two kinds of work:

- **Skills.** A skill is a small sandboxed program — a WebAssembly capsule — that the agent
  runs to perform a task. Capsules run inside strict limits (bounded memory and a compute
  deadline that actually traps runaway code), so a skill cannot exhaust the machine or
  escape its sandbox.
- **Code tasks.** The agent can run code and tools (file operations, shell commands) to get
  work done.

Every consequential action stops for a human under HIC-1:

- If a skill or agent wants to make an **on-chain** change, that becomes an intent in the
  SignatureCeremony. You see what it would do and decide. The agent holds no key, so it
  cannot make the change itself — only your approval, and the ceremony's signature, can.
- If the agent wants to run a **code or shell** task, that stops for an explicit approve or
  reject. There is no automatic run for code.

Budgeted autonomy (HIC-2) is available for on-chain actions through a grant, but code
execution always stops for a person. The result is an assistant that can do real work at
machine speed where it is safe to, and that cannot quietly move money or run commands
behind your back.
