//! L1-WP02 pin tests: the ACP SDK is one exact release, and the protocol version is the stable one.

use std::path::Path;

fn read(rel: &str) -> String {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join(rel);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()))
}

#[test]
fn acp_crate_is_exact_pin() {
    let manifest = read("Cargo.toml");
    assert!(
        manifest.contains("agent-client-protocol = \"=5.6.1\""),
        "the SDK must be pinned with = in Cargo.toml"
    );
    let lock = read("../../Cargo.lock");
    let versions: Vec<&str> = lock
        .split("[[package]]")
        .filter(|block| block.contains("\nname = \"agent-client-protocol\"\n"))
        .filter_map(|block| {
            block
                .lines()
                .find_map(|l| l.strip_prefix("version = \"")?.strip_suffix('"'))
        })
        .collect();
    assert_eq!(versions, vec![citrate_acp_host::pin::ACP_CRATE_VERSION]);
}

#[test]
fn protocol_version_is_stable_v1() {
    assert_eq!(citrate_acp_host::pin::protocol_version(), 1);
}
