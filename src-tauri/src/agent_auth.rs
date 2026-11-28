//! L1-WP14a (planset v060, lanes/L1_AGENT_RUNTIME.md WP14): one private home per ACP agent, the
//! environment an agent process starts with, and sign-out and removal.
//!
//! Every agent Core runs (Hermes, Codex, Claude Agent, Gemini CLI, goose, Grok Build, Muse Code)
//! keeps its own state under `<app data>/agents/<id>/home`, created owner-only (`0700`) at every
//! level. The agent's `HOME`, its XDG folders and the variables its own CLI reads for its config
//! folder (`CODEX_HOME`, `CLAUDE_CONFIG_DIR`, `GEMINI_CLI_HOME`) all point inside that home, so an
//! agent's own sign-in (ChatGPT for Codex, Google for Gemini CLI) is written there and nowhere
//! else, and one agent's home never holds another agent's state.
//!
//! Rules (BYOK ADR P-2, P-6, P-8; lane rules in WP14):
//! - **No vendor key ever reaches an agent.** The environment starts empty ([`apply_env`] clears
//!   the inherited one) and takes only [`INHERITED_ENV`] names from the launch environment; any
//!   vendor key variable ([`crate::hermes::cloud::VENDOR_KEY_ENV`]) and any value shaped like a
//!   vendor key is dropped. A proxy-based agent gets exactly one credential: a scoped Citrate
//!   route token (`cmp_<hex>`, [`ModelProxy::mint_route`]) in [`ROUTE_TOKEN_ENV`], bound to one
//!   provider, model and agent session label. The key stays in the keychain, read by the proxy.
//! - **Arguments never carry a credential**, not even a route token ([`check_argv`]): argv is
//!   visible to every local process.
//! - **ACP `authenticate` never carries a vendor key** ([`check_authenticate`]).
//! - **Core never reads an agent's login files.** [`AgentHomes::status`] looks at metadata only.
//! - **Sign-out** revokes the agent's routes and empties its home; **removal** revokes its routes
//!   and deletes `<app data>/agents/<id>` (the uninstall cleanup). Neither follows a symlink out of
//!   the agent's folder, and neither touches another agent.
//! - **Cross-agent denial**: [`AgentHomes::resolve_in_home`] answers a path only when it resolves
//!   inside that agent's own home; another agent's home is refused by name.
//!
//! What this module does not do yet (honest scope): it does not spawn agents (the ACP host,
//! WP03, calls [`AgentHomes::launch_env`] and [`apply_env`]), and the OS-level boundary between
//! agents is the L2 sandbox; until it lands, isolation is by folder permissions and these checks.
//! Removal is an ordinary delete; the E4.3 secure-delete routine replaces it when it lands.
//! Agents that keep their sign-in in the OS keychain instead of a file (Claude Code on macOS) are
//! not isolated by the home, which is why Claude Agent signs in only through the Citrate proxy
//! (owner answer L1-Q3: API key only) and [`HomeLayout::own_login`] is false for it.
//!
//! Data sources (Rule 7): the agent id comes from `release/agents.lock.json` (WP13) through the
//! ACP host; the home is core's app data folder; route tokens come from the in-process model
//! proxy. Keyless: nothing here signs or holds a vendor key (Rule 3).

// RED (L1-WP14a): the tests below are committed before the implementation.

#[cfg(test)]
mod tests {
    include!("agent_auth_tests.rs");
}
