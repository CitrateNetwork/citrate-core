//! The network this build targets (K6 network switch).
//!
//! # Why this module exists
//!
//! The values that say which network the app talks to were spread over five places: the chain
//! id and RPC in `citrate-core-kit::rpc`, the genesis in the generated address book, the
//! bootnodes in `config/member-node.toml`, and the explorer and faucet URLs in their own
//! modules. The 2026-12-01 genesis keeps chain id 40204 (C2 decision D1, recommended), so a
//! build for the old network and one for the new differ only in those values, and one stale
//! copy is enough to sync or sign against the wrong chain.
//!
//! `network/profiles.json` is now the one source. It is embedded at compile time and read by
//! this module and by the webview (`src/network/profile.ts`). The active profile drives:
//! - the node: its bootnodes, chain id and genesis profile in the rendered `node.toml`, its
//!   `--network`, the validator overrides it is (or is not) given, and the genesis its data dir
//!   must belong to ([`crate::node_genesis`]);
//! - the network gate on every signature and send ([`install_network_gate`], checked by
//!   `citrate-core-kit::rpc::RpcClient::verify_network`): the RPC's chain id AND block-0 hash
//!   must match before anything is signed or sent.
//!
//! # Not set means refuse
//!
//! A value nobody has decided yet (for the new genesis: everything, until the cut) is `null` in
//! the JSON with a reason in `pending`. It is never a placeholder that looks real. A profile
//! with any `null` is not ready: [`NetworkProfile::ready`] returns [`ProfileError::NotSet`]
//! naming each missing value, the node is not started, and the network gate refuses every
//! signature. The tripwire tests below keep the other copies (address book, kit constants,
//! explorer and faucet URLs) equal to the active profile until they read from here.

use std::collections::BTreeMap;
use std::sync::OnceLock;

/// The embedded profiles.
const PROFILES_JSON: &str = include_str!("../network/profiles.json");

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProfilesFile {
    active: String,
    node_genesis_profiles: BTreeMap<String, RawNodeGenesis>,
    profiles: BTreeMap<String, RawProfile>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RawNodeGenesis {
    genesis_hash: String,
    node_default: bool,
    compiled_activations: bool,
}

/// A genesis profile the bundled node builds (`nodeGenesisProfiles` in `profiles.json`): the node
/// is told it with `[chain] genesis_profile` and builds this block 0 (citrate-chain
/// `node/src/genesis.rs` `initialize_genesis_state_with_profile`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeGenesis {
    /// The `[chain] genesis_profile` name.
    pub name: String,
    /// The block 0 the node builds for it on chain 40204 (`0x` + 64 lowercase hex).
    pub genesis_hash: String,
    /// The node also builds it when the config names no genesis profile (its chain-id default).
    pub node_default: bool,
    /// The genesis compiles its ValidatorRegistry and activation height (citrate-chain
    /// `genesis::resolve_validator_registry`): the app passes no `CITRATE_VALIDATOR_*` override,
    /// because the node refuses one that differs from the compiled values.
    pub compiled_activations: bool,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RawProfile {
    label: String,
    chain_id: Option<u64>,
    genesis_hash: Option<String>,
    genesis_profile: Option<String>,
    node_network: Option<String>,
    rpc_url: Option<String>,
    explorer_url: Option<String>,
    faucet_url: Option<String>,
    bootnodes: Option<Vec<String>>,
    address_book: Option<String>,
    address_book_sha256: Option<String>,
    min_chain_rev: Option<String>,
    #[serde(default)]
    member_prune_retain: Option<u64>,
    #[serde(default)]
    retired: Option<String>,
    #[serde(default)]
    node_checkpoint: Option<RawNodeCheckpoint>,
    #[serde(default)]
    pending: BTreeMap<String, String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RawNodeCheckpoint {
    interval: u64,
    committee_size: u64,
    vote_depth: u64,
}

/// The `[checkpoint]` values a node config must repeat on a genesis that compiles them (r1007).
///
/// The citrate-chain node compiles chain parameters for such a genesis and refuses to start when
/// the config's `[checkpoint] interval`, `committee_size` or `vote_depth` differ from them (a
/// per-node value would make the node propose, select and vote on other checkpoints than the
/// fleet). `scripts/network-cut.py apply` writes them into the profile from the cut's
/// `chainParams`, and [`render_node_network`] writes all three into `node.toml` explicitly. A
/// profile without them (alpha-r1005, r1006) keeps the template's section.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NodeCheckpoint {
    /// `[checkpoint] interval` (the chain's `checkpoint_interval`).
    pub interval: u64,
    /// `[checkpoint] committee_size`.
    pub committee_size: u64,
    /// `[checkpoint] vote_depth` (the chain's `checkpoint_vote_depth`).
    pub vote_depth: u64,
}

/// The fewest blocks a member node may keep when it prunes: the undo window (ADR-2026-11-03
/// CV-7a, undo window = retention; citrate-chain r1007 `REORG_UNDO_DEPTH` and `MAX_REORG_DEPTH`
/// are 10,000). A node that pruned inside it could not undo a reorg it must follow.
pub const MIN_MEMBER_PRUNE_RETAIN: u64 = 10_000;

/// One network profile as written in `profiles.json`. `None` = not set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkProfile {
    /// The profile key (`alpha-r1005`, `genesis-2026-12`).
    pub id: String,
    /// The name shown to people.
    pub label: String,
    pub chain_id: Option<u64>,
    pub genesis_hash: Option<String>,
    /// The `[chain] genesis_profile` the node config names: which block 0 the node builds. Must
    /// be one the bundled node builds ([`node_genesis`]).
    pub genesis_profile: Option<String>,
    /// The node's `--network` argument.
    pub node_network: Option<String>,
    pub rpc_url: Option<String>,
    pub explorer_url: Option<String>,
    pub faucet_url: Option<String>,
    pub bootnodes: Option<Vec<String>>,
    /// The generated address book, relative to `src-tauri/`.
    pub address_book: Option<String>,
    /// sha256 (64 lowercase hex) of the frozen canonical citrate-chain address book this
    /// profile's genesis was deployed with, as exported at the reroll.
    pub address_book_sha256: Option<String>,
    /// The citrate-chain commit (40 lowercase hex) the bundled node must contain for this
    /// network; `MIN_CHAIN_REV` in `scripts/build-sidecar.sh` pins the active one.
    pub min_chain_rev: Option<String>,
    /// CORE-N5: the blocks a member node keeps (`[storage] pruning = true`, `keep_blocks` = this);
    /// `None` = full history (the template). Never below [`MIN_MEMBER_PRUNE_RETAIN`].
    pub member_prune_retain: Option<u64>,
    /// CORE-N2: why this network is retired. A retired profile stays known (a data dir from it is
    /// still named, [`profile_for_genesis`]) but is refused like an unset one ([`Self::ready`]).
    pub retired: Option<String>,
    /// The `[checkpoint]` values the node config repeats; `None` = the template's section.
    pub node_checkpoint: Option<NodeCheckpoint>,
    /// For each unset field: what sets it.
    pub pending: BTreeMap<String, String>,
}

/// A profile with every value set and checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadyNetwork {
    pub id: String,
    pub label: String,
    pub chain_id: u64,
    /// `0x` + 64 lowercase hex, not zero.
    pub genesis_hash: String,
    /// The genesis profile the node is told to build, one the bundled node builds.
    pub node_genesis: NodeGenesis,
    pub node_network: String,
    pub rpc_url: String,
    pub explorer_url: String,
    pub faucet_url: String,
    /// At least one, each `noise_<64 hex>@<host>:<port>`.
    pub bootnodes: Vec<String>,
    pub address_book: String,
    /// 64 lowercase hex, not zero.
    pub address_book_sha256: String,
    /// 40 lowercase hex, not zero.
    pub min_chain_rev: String,
    /// The blocks a member node keeps, or `None` for full history (at least
    /// [`MIN_MEMBER_PRUNE_RETAIN`]).
    pub member_prune_retain: Option<u64>,
    /// The `[checkpoint]` values written into the node config, or `None` for the template's.
    pub node_checkpoint: Option<NodeCheckpoint>,
}

/// Why a profile cannot be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProfileError {
    /// The embedded `profiles.json` does not parse (a build defect).
    Malformed(String),
    /// `active` names a profile that does not exist.
    UnknownProfile(String),
    /// Values are not set yet: `(field, what sets it)` for each.
    NotSet {
        profile: String,
        missing: Vec<(String, String)>,
    },
    /// The network is retired (CORE-N2): known, but no node starts on it and nothing is signed.
    Retired { profile: String, why: String },
    /// A value is set but not well formed.
    Invalid {
        profile: String,
        field: &'static str,
        why: String,
    },
}

impl std::fmt::Display for ProfileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProfileError::Malformed(e) => {
                write!(f, "this build's network profiles cannot be read: {e}")
            }
            ProfileError::UnknownProfile(p) => {
                write!(
                    f,
                    "this build names network profile {p:?}, which it does not contain"
                )
            }
            ProfileError::NotSet { profile, missing } => {
                let names: Vec<&str> = missing.iter().map(|(k, _)| k.as_str()).collect();
                write!(
                    f,
                    "this build targets network {profile:?}, whose {} not set yet; nothing is \
                     synced or signed until they are",
                    if names.len() == 1 {
                        format!("{} is", names[0])
                    } else {
                        format!("{} are", names.join(", "))
                    }
                )
            }
            ProfileError::Retired { profile, why } => write!(
                f,
                "network {profile:?} is retired ({why}); this build does not start a node or \
                 sign on it"
            ),
            ProfileError::Invalid {
                profile,
                field,
                why,
            } => write!(
                f,
                "network profile {profile:?} has an invalid {field}: {why}"
            ),
        }
    }
}

impl std::error::Error for ProfileError {}

fn parsed() -> &'static Result<(String, Vec<NetworkProfile>), ProfileError> {
    static PARSED: OnceLock<Result<(String, Vec<NetworkProfile>), ProfileError>> = OnceLock::new();
    PARSED.get_or_init(|| parse(PROFILES_JSON))
}

fn parsed_node_genesis() -> &'static Result<Vec<NodeGenesis>, ProfileError> {
    static PARSED: OnceLock<Result<Vec<NodeGenesis>, ProfileError>> = OnceLock::new();
    PARSED.get_or_init(|| parse_node_genesis(PROFILES_JSON))
}

/// A genesis profile name the node config can carry: lowercase letters, digits and `_` (the
/// names citrate-chain compiles), so it is written into the TOML string as is.
pub fn is_genesis_profile_name(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

fn parse_node_genesis(json: &str) -> Result<Vec<NodeGenesis>, ProfileError> {
    let file: ProfilesFile =
        serde_json::from_str(json).map_err(|e| ProfileError::Malformed(e.to_string()))?;
    file.node_genesis_profiles
        .into_iter()
        .map(|(name, g)| {
            if !is_genesis_profile_name(&name) {
                return Err(ProfileError::Malformed(format!(
                    "nodeGenesisProfiles: {name:?} is not a genesis profile name"
                )));
            }
            let genesis_hash = check_hash(&g.genesis_hash).ok_or_else(|| {
                ProfileError::Malformed(format!(
                    "nodeGenesisProfiles.{name}: {:?} is not a block hash",
                    g.genesis_hash
                ))
            })?;
            Ok(NodeGenesis {
                name,
                genesis_hash,
                node_default: g.node_default,
                compiled_activations: g.compiled_activations,
            })
        })
        .collect()
}

/// The genesis profiles the bundled node builds (`nodeGenesisProfiles`).
pub fn node_genesis_profiles() -> Result<&'static [NodeGenesis], ProfileError> {
    parsed_node_genesis()
        .as_ref()
        .map(Vec::as_slice)
        .map_err(Clone::clone)
}

/// The genesis profile `name`, when the bundled node builds it. (Readiness looks it up in the
/// table it is given, [`NetworkProfile::ready_with`]; this is the embedded table's lookup.)
#[cfg(test)]
pub fn node_genesis(name: &str) -> Option<&'static NodeGenesis> {
    node_genesis_profiles()
        .ok()?
        .iter()
        .find(|g| g.name == name)
}

fn parse(json: &str) -> Result<(String, Vec<NetworkProfile>), ProfileError> {
    let file: ProfilesFile =
        serde_json::from_str(json).map_err(|e| ProfileError::Malformed(e.to_string()))?;
    let profiles = file
        .profiles
        .into_iter()
        .map(|(id, p)| NetworkProfile {
            id,
            label: p.label,
            chain_id: p.chain_id,
            genesis_hash: p.genesis_hash,
            genesis_profile: p.genesis_profile,
            node_network: p.node_network,
            rpc_url: p.rpc_url,
            explorer_url: p.explorer_url,
            faucet_url: p.faucet_url,
            bootnodes: p.bootnodes,
            address_book: p.address_book,
            address_book_sha256: p.address_book_sha256,
            min_chain_rev: p.min_chain_rev,
            member_prune_retain: p.member_prune_retain,
            retired: p.retired,
            node_checkpoint: p.node_checkpoint.map(|c| NodeCheckpoint {
                interval: c.interval,
                committee_size: c.committee_size,
                vote_depth: c.vote_depth,
            }),
            pending: p.pending,
        })
        .collect();
    Ok((file.active, profiles))
}

/// Every profile this build knows.
pub fn profiles() -> Result<&'static [NetworkProfile], ProfileError> {
    parsed()
        .as_ref()
        .map(|(_, p)| p.as_slice())
        .map_err(Clone::clone)
}

/// The profile with this id.
pub fn profile(id: &str) -> Result<&'static NetworkProfile, ProfileError> {
    profiles()?
        .iter()
        .find(|p| p.id == id)
        .ok_or_else(|| ProfileError::UnknownProfile(id.to_string()))
}

/// The id of the network this build targets.
pub fn active_id() -> Result<&'static str, ProfileError> {
    parsed()
        .as_ref()
        .map(|(a, _)| a.as_str())
        .map_err(Clone::clone)
}

/// The profile of the network this build targets (possibly not ready).
pub fn active() -> Result<&'static NetworkProfile, ProfileError> {
    profile(active_id()?)
}

/// The known profile whose genesis is `genesis` (case-insensitive), retired or not: how a node
/// data dir from an earlier network is named (CORE-N3, `chain.<profile>.bak`).
pub fn profile_for_genesis(genesis: &str) -> Option<&'static NetworkProfile> {
    profiles().ok()?.iter().find(|p| {
        p.genesis_hash
            .as_deref()
            .is_some_and(|g| g.eq_ignore_ascii_case(genesis))
    })
}

/// The network this build targets, with every value set and checked. Fails closed.
pub fn active_ready() -> Result<ReadyNetwork, ProfileError> {
    active()?.ready()
}

/// `0x` + 64 hex, not all zero; returned lowercase.
fn check_hash(s: &str) -> Option<String> {
    let hex = s.strip_prefix("0x")?;
    if hex.len() != 64 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    if hex.bytes().all(|b| b == b'0') {
        return None;
    }
    Some(format!("0x{}", hex.to_ascii_lowercase()))
}

/// Exactly `len` lowercase hex digits, not all zero (a digest or a full commit id).
fn is_lower_hex_nonzero(s: &str, len: usize) -> bool {
    s.len() == len
        && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
        && s.bytes().any(|b| b != b'0')
}

/// `noise_<64 hex>@<host>:<port>`, the form the node verifies at the Noise handshake.
pub fn is_bootnode(s: &str) -> bool {
    let Some(rest) = s.strip_prefix("noise_") else {
        return false;
    };
    let Some((key, addr)) = rest.split_once('@') else {
        return false;
    };
    let Some((host, port)) = addr.rsplit_once(':') else {
        return false;
    };
    key.len() == 64
        && key.bytes().all(|b| b.is_ascii_hexdigit())
        && !host.is_empty()
        && host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
        && port.parse::<u16>().map(|p| p > 0).unwrap_or(false)
}

impl NetworkProfile {
    /// The unset fields, each with what sets it (`"not recorded"` when `pending` has no entry).
    pub fn missing(&self) -> Vec<(String, String)> {
        let mut out = Vec::new();
        let mut miss = |name: &str, unset: bool| {
            if unset {
                let why = self
                    .pending
                    .get(name)
                    .cloned()
                    .unwrap_or_else(|| "not recorded".to_string());
                out.push((name.to_string(), why));
            }
        };
        miss("chainId", self.chain_id.is_none());
        miss("genesisHash", self.genesis_hash.is_none());
        miss("genesisProfile", self.genesis_profile.is_none());
        miss("nodeNetwork", self.node_network.is_none());
        miss("rpcUrl", self.rpc_url.is_none());
        miss("explorerUrl", self.explorer_url.is_none());
        miss("faucetUrl", self.faucet_url.is_none());
        miss("bootnodes", self.bootnodes.is_none());
        miss("addressBook", self.address_book.is_none());
        miss("addressBookSha256", self.address_book_sha256.is_none());
        miss("minChainRev", self.min_chain_rev.is_none());
        out
    }

    /// Every value set and well formed, or why not.
    pub fn ready(&self) -> Result<ReadyNetwork, ProfileError> {
        self.ready_with(node_genesis_profiles().unwrap_or(&[]))
    }

    /// [`Self::ready`] against a given table of the genesis profiles the bundled node builds (the
    /// embedded `nodeGenesisProfiles` for [`Self::ready`]; tests pass the table of a profiles file
    /// as the network cut writes it).
    pub fn ready_with(&self, node_table: &[NodeGenesis]) -> Result<ReadyNetwork, ProfileError> {
        let missing = self.missing();
        if !missing.is_empty() {
            return Err(ProfileError::NotSet {
                profile: self.id.clone(),
                missing,
            });
        }
        let invalid = |field: &'static str, why: String| ProfileError::Invalid {
            profile: self.id.clone(),
            field,
            why,
        };
        if let Some(why) = &self.retired {
            if why.trim().is_empty() {
                return Err(invalid("retired", "an empty reason".into()));
            }
            return Err(ProfileError::Retired {
                profile: self.id.clone(),
                why: why.clone(),
            });
        }
        if let Some(n) = self.member_prune_retain {
            if n < MIN_MEMBER_PRUNE_RETAIN {
                return Err(invalid(
                    "memberPruneRetain",
                    format!(
                        "{n} is below the {MIN_MEMBER_PRUNE_RETAIN}-block undo window (ADR CV-7a)"
                    ),
                ));
            }
        }
        // `missing()` is empty, so every Option below is Some; `unwrap_or_default` keeps the
        // code panic-free and an empty value is refused by the checks that follow.
        let chain_id = self.chain_id.unwrap_or_default();
        if chain_id == 0 {
            return Err(invalid("chainId", "0 is not a chain id".into()));
        }
        let raw_genesis = self.genesis_hash.clone().unwrap_or_default();
        let genesis_hash = check_hash(&raw_genesis).ok_or_else(|| {
            invalid(
                "genesisHash",
                format!("{raw_genesis:?} is not a block hash"),
            )
        })?;
        // Fail closed: a genesis profile the bundled node does not build is refused here, so no
        // node starts (the node would refuse it too, or an older one fall back to the chain-id
        // default genesis) and nothing is signed.
        let raw_gp = self.genesis_profile.clone().unwrap_or_default();
        let node_genesis = node_table
            .iter()
            .find(|g| g.name == raw_gp)
            .cloned()
            .ok_or_else(|| {
                let known: Vec<String> = node_table.iter().map(|g| g.name.clone()).collect();
                invalid(
                    "genesisProfile",
                    format!(
                    "{raw_gp:?} is not a genesis profile the bundled node builds (it builds: {})",
                    known.join(", ")
                ),
                )
            })?;
        let node_network = self.node_network.clone().unwrap_or_default();
        if node_network.is_empty()
            || !node_network
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        {
            return Err(invalid("nodeNetwork", format!("{node_network:?}")));
        }
        let https = |field: &'static str, v: &Option<String>| -> Result<String, ProfileError> {
            let v = v.clone().unwrap_or_default();
            match v.strip_prefix("https://") {
                Some(host) if !host.is_empty() && !host.contains(char::is_whitespace) => Ok(v),
                _ => Err(invalid(field, format!("{v:?} is not an https URL"))),
            }
        };
        let rpc_url = https("rpcUrl", &self.rpc_url)?;
        let explorer_url = https("explorerUrl", &self.explorer_url)?;
        let faucet_url = https("faucetUrl", &self.faucet_url)?;
        let bootnodes = self.bootnodes.clone().unwrap_or_default();
        if bootnodes.is_empty() {
            return Err(invalid("bootnodes", "the list is empty".into()));
        }
        if let Some(bad) = bootnodes.iter().find(|b| !is_bootnode(b)) {
            return Err(invalid("bootnodes", format!("{bad:?}")));
        }
        let address_book = self.address_book.clone().unwrap_or_default();
        if address_book.is_empty() {
            return Err(invalid("addressBook", "empty".into()));
        }
        let address_book_sha256 = self.address_book_sha256.clone().unwrap_or_default();
        if !is_lower_hex_nonzero(&address_book_sha256, 64) {
            return Err(invalid(
                "addressBookSha256",
                format!("{address_book_sha256:?} is not a sha256 digest (64 lowercase hex)"),
            ));
        }
        let min_chain_rev = self.min_chain_rev.clone().unwrap_or_default();
        if !is_lower_hex_nonzero(&min_chain_rev, 40) {
            return Err(invalid(
                "minChainRev",
                format!("{min_chain_rev:?} is not a commit (40 lowercase hex)"),
            ));
        }
        if let Some(c) = self.node_checkpoint {
            if c.interval == 0 || c.committee_size == 0 || c.vote_depth == 0 {
                return Err(invalid(
                    "nodeCheckpoint",
                    format!(
                        "interval {}, committeeSize {}, voteDepth {}: each must be positive",
                        c.interval, c.committee_size, c.vote_depth
                    ),
                ));
            }
        }
        Ok(ReadyNetwork {
            id: self.id.clone(),
            label: self.label.clone(),
            chain_id,
            genesis_hash,
            node_genesis,
            node_network,
            rpc_url,
            explorer_url,
            faucet_url,
            bootnodes,
            address_book,
            address_book_sha256,
            min_chain_rev,
            member_prune_retain: self.member_prune_retain,
            node_checkpoint: self.node_checkpoint,
        })
    }

    /// What the network gate checks for this profile: its chain id and genesis only when the
    /// WHOLE profile is ready ([`Self::ready`]). Any value not set or malformed gives an all-unset
    /// gate, which refuses every wallet signature (transaction, message, typed data, sign-in) and
    /// every send.
    pub fn expected_network(&self) -> crate::rpc::ExpectedNetwork {
        match self.ready() {
            Ok(r) => crate::rpc::ExpectedNetwork {
                name: r.id,
                chain_id: Some(r.chain_id),
                genesis_hash: Some(r.genesis_hash),
            },
            Err(e) => {
                eprintln!("[network] {e}");
                crate::rpc::ExpectedNetwork {
                    name: self.id.clone(),
                    chain_id: None,
                    genesis_hash: None,
                }
            }
        }
    }
}

/// The gate for the active profile ([`NetworkProfile::expected_network`]); all-unset when the
/// profiles cannot be read or the active one is unknown.
pub fn active_gate() -> crate::rpc::ExpectedNetwork {
    match active() {
        Ok(p) => p.expected_network(),
        Err(e) => {
            eprintln!("[network] {e}");
            crate::rpc::ExpectedNetwork {
                name: "unknown".into(),
                chain_id: None,
                genesis_hash: None,
            }
        }
    }
}

/// Install the process-wide network gate for the active profile (app startup), from
/// [`active_gate`]. When the profiles cannot be read, the active one is unknown, or any of its
/// values is not set, an all-unset gate is installed, so every signature and send is refused
/// rather than left unchecked.
pub fn install_network_gate() {
    let expected = active_gate();
    if !expected.is_set() {
        eprintln!(
            "[network] network {:?} is not set in this build; signing and sending are refused",
            expected.name
        );
    }
    if !crate::rpc::install_network_gate(expected) {
        eprintln!("[network] the network gate was already installed");
    }
}

/// The chain id placeholder in the member node config template (quoted, so the template itself
/// stays valid TOML; it is replaced, quotes included, by the number).
pub const CHAIN_ID_PLACEHOLDER: &str = "\"{{CHAIN_ID}}\"";
/// The bootnode list placeholder: one whole array-entry line in the template.
pub const BOOTNODES_PLACEHOLDER: &str = "    \"{{BOOTSTRAP_NODES}}\",";
/// The genesis profile placeholder (inside a quoted TOML string; names are checked by
/// [`is_genesis_profile_name`], so they need no escaping).
pub const GENESIS_PROFILE_PLACEHOLDER: &str = "{{GENESIS_PROFILE}}";

/// Fill the network values of the member node config template from `net`. The bootnode list is
/// written one quoted entry per line (values are checked by [`NetworkProfile::ready`], so no
/// quoting can be broken).
///
/// CORE-N5: a profile with a member retention ([`ReadyNetwork::member_prune_retain`]) gets a pruned
/// chain store keeping that many blocks (`[storage] pruning = true`, `keep_blocks`); without one
/// the template's full-history values are kept.
///
/// A profile with [`ReadyNetwork::node_checkpoint`] (the r1007 cut) gets `[checkpoint] interval`,
/// `committee_size` and `vote_depth` written explicitly, the values its genesis compiles, so the
/// node's start-up check passes and no node default decides them; without it the template's
/// `[checkpoint]` section is kept byte for byte.
pub fn render_node_network(template: &str, net: &ReadyNetwork) -> String {
    let nodes: Vec<String> = net
        .bootnodes
        .iter()
        .map(|b| format!("    \"{b}\","))
        .collect();
    let out = template
        .replace(CHAIN_ID_PLACEHOLDER, &net.chain_id.to_string())
        .replace(GENESIS_PROFILE_PLACEHOLDER, &net.node_genesis.name)
        .replace(BOOTNODES_PLACEHOLDER, &nodes.join("\n"));
    let out = match net.member_prune_retain {
        Some(keep) => set_table_keys(
            &out,
            "storage",
            &[
                ("pruning", "true".into()),
                ("keep_blocks", keep.to_string()),
            ],
        ),
        None => out,
    };
    match net.node_checkpoint {
        Some(c) => set_table_keys(
            &out,
            "checkpoint",
            &[
                ("interval", c.interval.to_string()),
                ("committee_size", c.committee_size.to_string()),
                ("vote_depth", c.vote_depth.to_string()),
            ],
        ),
        None => out,
    }
}

/// `key = value` for each of `keys` inside the `[table]` of a rendered config: a key already in
/// the table is rewritten in place, one that is not is added after the table's last key line.
/// Every other line is kept byte for byte.
fn set_table_keys(config: &str, table: &str, keys: &[(&str, String)]) -> String {
    let header = format!("[{table}]");
    let mut in_table = false;
    let mut seen = vec![false; keys.len()];
    let mut out: Vec<String> = Vec::new();
    // Index in `out` after which keys missing from the table are inserted.
    let mut last_key_line: Option<usize> = None;
    for line in config.split('\n') {
        let t = line.trim_start();
        if t.starts_with('[') {
            in_table = t.starts_with(&header);
            if in_table {
                last_key_line = Some(out.len());
            }
        }
        let key = t.split('=').next().unwrap_or("").trim();
        let hit = if in_table && t.contains('=') && !t.starts_with('#') {
            keys.iter().position(|(k, _)| *k == key)
        } else {
            None
        };
        match hit {
            Some(i) => {
                seen[i] = true;
                out.push(format!("{} = {}", keys[i].0, keys[i].1));
            }
            None => out.push(line.to_string()),
        }
        if in_table && t.contains('=') && !t.starts_with('#') {
            last_key_line = Some(out.len() - 1);
        }
    }
    let missing: Vec<String> = keys
        .iter()
        .zip(&seen)
        .filter(|(_, s)| !**s)
        .map(|((k, v), _)| format!("{k} = {v}"))
        .collect();
    if !missing.is_empty() {
        if let Some(at) = last_key_line {
            out.splice(at + 1..at + 1, missing);
        }
    }
    out.join("\n")
}

#[cfg(test)]
mod tests {
    include!("network_profile_tests.rs");
}
