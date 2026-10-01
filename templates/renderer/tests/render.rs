//! HUP-S6.2: rendering the shipped templates, and the renderer's refusal paths.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use citrate_templates::budget::Tier;
use citrate_templates::{RenderError, TemplateSet};

const OWNER: &str = "0x5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed";

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

/// A fresh, not-yet-existing directory under the system temp dir.
struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        static N: AtomicU64 = AtomicU64::new(0);
        let n = N.fetch_add(1, Ordering::SeqCst);
        let p = std::env::temp_dir().join(format!(
            "n3-templates-test-{}-{}-{tag}",
            std::process::id(),
            n
        ));
        let _ = fs::remove_dir_all(&p);
        Scratch(p)
    }
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn set() -> TemplateSet {
    match TemplateSet::open(&root()) {
        Ok(s) => s,
        Err(e) => panic!("shipped templates failed to open: {e}"),
    }
}

fn params(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

fn lemon() -> BTreeMap<String, String> {
    params(&[
        ("name", "Lemon Drops"),
        ("symbol", "LEMON"),
        ("supply", "500"),
        ("price", "5000000000000000000"),
        ("owner", &OWNER.to_lowercase()),
    ])
}

fn read(p: &Path) -> String {
    match fs::read_to_string(p) {
        Ok(s) => s,
        Err(e) => panic!("read {}: {e}", p.display()),
    }
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, out);
        } else {
            out.push(p);
        }
    }
}

#[test]
fn the_shipped_template_ids_are_listed() {
    let ids: Vec<String> = set().ids().map(str::to_string).collect();
    assert_eq!(
        ids,
        [
            "erc1155",
            "erc20",
            "erc721",
            "erc721-solady",
            "governor",
            "hello-mint"
        ]
    );
}

#[test]
fn every_template_renders_with_no_placeholder_left() {
    let s = set();
    let ids: Vec<String> = s.ids().map(str::to_string).collect();
    for id in ids {
        let out = Scratch::new(&id);
        let manifest = match s.manifest(&id) {
            Some(m) => m,
            None => panic!("{id} has no manifest"),
        };
        let mut p = lemon();
        p.retain(|k, _| manifest.params.contains_key(k));
        let report = match s.render(&id, &p, Tier::T1, out.path()) {
            Ok(r) => r,
            Err(e) => panic!("{id}: {e}"),
        };
        assert_eq!(report.template, id);
        assert!(!report.files.is_empty());
        let mut files = Vec::new();
        walk(out.path(), &mut files);
        for f in files {
            let body = read(&f);
            assert!(
                !body.contains("{{ct:"),
                "{} keeps a placeholder",
                f.display()
            );
        }
        assert!(
            out.path().join("citrate-template.lock.json").is_file(),
            "{id}"
        );
    }
}

#[test]
fn erc721_bakes_the_validated_parameters() {
    let out = Scratch::new("erc721-bake");
    if let Err(e) = set().render("erc721", &lemon(), Tier::T1, out.path()) {
        panic!("{e}");
    }
    let src = read(&out.path().join("src/Token.sol"));
    assert!(src.contains("contract LemonDrops is"));
    assert!(src.contains("ERC721(\"Lemon Drops\", \"LEMON\")"));
    assert!(src.contains("MAX_SUPPLY = 500;"));
    assert!(src.contains("PRICE = 5000000000000000000;"));
    // The owner literal is EIP-55 checksummed (solc rejects anything else).
    assert!(src.contains(&format!("Ownable({OWNER})")));
}

#[test]
fn medusa_config_carries_the_tier_budget() {
    let s = set();
    let budgets = s.budgets().clone();
    for tier in [Tier::T0, Tier::T1, Tier::T2] {
        let out = Scratch::new(&format!("medusa-{}", tier.id()));
        let report = match s.render("erc721", &lemon(), tier, out.path()) {
            Ok(r) => r,
            Err(e) => panic!("{e}"),
        };
        let raw = read(&out.path().join("medusa.json"));
        let v: serde_json::Value = match serde_json::from_str(&raw) {
            Ok(v) => v,
            Err(e) => panic!("medusa.json is not JSON: {e}"),
        };
        let b = budgets.for_tier(tier);
        assert_eq!(v["fuzzing"]["testLimit"].as_u64(), Some(b.test_limit));
        assert_eq!(v["fuzzing"]["workers"].as_u64(), Some(u64::from(b.workers)));
        assert_eq!(
            v["fuzzing"]["callSequenceLength"].as_u64(),
            Some(u64::from(b.call_sequence_length))
        );
        assert_eq!(v["fuzzing"]["timeout"].as_u64(), Some(b.timeout_secs));
        assert_eq!(
            v["fuzzing"]["targetContracts"][0].as_str(),
            Some("LemonDropsProperties")
        );
        assert_eq!(
            v["fuzzing"]["chainConfig"]["cheatCodes"]["enableFFI"].as_bool(),
            Some(false)
        );
        assert_eq!(report.medusa_budget.as_ref(), Some(b));
    }
}

#[test]
fn the_lock_file_records_template_digest_tier_and_params() {
    let a = Scratch::new("lock-a");
    let b = Scratch::new("lock-b");
    let s = set();
    let ra = match s.render(
        "erc20",
        &params(&[
            ("name", "Lemon"),
            ("symbol", "LEM"),
            ("supply", "1000"),
            ("owner", OWNER),
        ]),
        Tier::T0,
        a.path(),
    ) {
        Ok(r) => r,
        Err(e) => panic!("{e}"),
    };
    let rb = match s.render(
        "erc20",
        &params(&[
            ("name", "Lime"),
            ("symbol", "LIM"),
            ("supply", "7"),
            ("owner", OWNER),
        ]),
        Tier::T0,
        b.path(),
    ) {
        Ok(r) => r,
        Err(e) => panic!("{e}"),
    };
    // The digest covers the template, not the parameters.
    assert_eq!(ra.digest, rb.digest);
    assert_eq!(ra.digest.len(), 64);
    let lock: serde_json::Value =
        match serde_json::from_str(&read(&a.path().join("citrate-template.lock.json"))) {
            Ok(v) => v,
            Err(e) => panic!("{e}"),
        };
    assert_eq!(lock["template"].as_str(), Some("erc20"));
    assert_eq!(lock["digest"].as_str(), Some(ra.digest.as_str()));
    assert_eq!(lock["tier"].as_str(), Some("T0"));
    assert_eq!(lock["params"]["name"].as_str(), Some("Lemon"));
    assert_eq!(lock["params"]["contract"].as_str(), Some("Lemon"));
    assert_eq!(
        lock["deps"]["openzeppelin-contracts"]["tag"].as_str(),
        Some("v5.7.0")
    );
}

#[test]
fn defaults_fill_missing_optional_params() {
    let out = Scratch::new("defaults");
    let p = params(&[
        ("name", "Lemon Drops"),
        ("symbol", "LEMON"),
        ("owner", OWNER),
    ]);
    let report = match set().render("erc721", &p, Tier::T1, out.path()) {
        Ok(r) => r,
        Err(e) => panic!("{e}"),
    };
    assert!(report.params.contains_key("supply"));
    assert!(report.params.contains_key("price"));
}

#[test]
fn refusals_write_nothing() {
    let s = set();
    let cases: Vec<(BTreeMap<String, String>, &str)> = vec![
        (
            {
                let mut p = lemon();
                p.insert(
                    "name".into(),
                    "X\"); selfdestruct(payable(msg.sender)); //".into(),
                );
                p
            },
            "injection through name",
        ),
        (
            {
                let mut p = lemon();
                p.remove("owner");
                p
            },
            "missing owner",
        ),
        (
            {
                let mut p = lemon();
                p.insert("base_uri".into(), "ipfs://x".into());
                p
            },
            "unknown key",
        ),
        (
            {
                let mut p = lemon();
                p.insert("supply".into(), "1000001".into());
                p
            },
            "supply above the template max",
        ),
        (
            {
                let mut p = lemon();
                p.insert("name".into(), "Ownable".into());
                p
            },
            "name shadows an import",
        ),
    ];
    for (p, why) in cases {
        let out = Scratch::new("refuse");
        let r = s.render("erc721", &p, Tier::T1, out.path());
        assert!(matches!(r, Err(RenderError::Param { .. })), "{why}: {r:?}");
        assert!(!out.path().exists(), "{why}: wrote output");
    }
    let out = Scratch::new("refuse-id");
    assert!(matches!(
        s.render("../erc721", &lemon(), Tier::T1, out.path()),
        Err(RenderError::UnknownTemplate(_))
    ));
}

#[test]
fn a_non_empty_output_directory_is_refused_and_left_alone() {
    let out = Scratch::new("nonempty");
    if let Err(e) = fs::create_dir_all(out.path()) {
        panic!("{e}");
    }
    let keep = out.path().join("keep.txt");
    if let Err(e) = fs::write(&keep, "mine") {
        panic!("{e}");
    }
    let r = set().render("erc721", &lemon(), Tier::T1, out.path());
    assert!(matches!(r, Err(RenderError::OutputNotEmpty(_))), "{r:?}");
    assert_eq!(read(&keep), "mine");
    assert!(!out.path().join("src").exists());
}

#[test]
fn hello_mint_is_the_erc721_contract_plus_the_app() {
    let out = Scratch::new("hello");
    let report = match set().render("hello-mint", &lemon(), Tier::T1, out.path()) {
        Ok(r) => r,
        Err(e) => panic!("{e}"),
    };
    assert_eq!(report.includes.len(), 1);
    assert!(out.path().join("contracts/src/Token.sol").is_file());
    assert!(out.path().join("contracts/medusa.json").is_file());
    assert!(out
        .path()
        .join("contracts/citrate-template.lock.json")
        .is_file());
    assert!(out.path().join("app/package.json").is_file());
    let collection = read(&out.path().join("app/src/collection.ts"));
    assert!(collection.contains("\"Lemon Drops\""));
    assert!(collection.contains("500n"));
    assert!(collection.contains("5000000000000000000n"));
}

/// The hello-mint app ships a hand-written ABI. Every function and error it names
/// must exist in BOTH ERC-721 variants, so the app works against either.
#[test]
fn the_app_abi_matches_both_erc721_variants() {
    let abi = read(&root().join("hello-mint/files/app/src/abi.ts"));
    let mut entries: Vec<(String, String)> = Vec::new();
    let mut kind = String::new();
    for line in abi.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("type: \"") {
            kind = rest.split('"').next().unwrap_or("").to_string();
        } else if let Some(rest) = t.strip_prefix("name: \"") {
            let name = rest.split('"').next().unwrap_or("").to_string();
            entries.push((kind.clone(), name));
        }
    }
    let functions = entries.iter().filter(|(k, _)| k == "function").count();
    let errors = entries.iter().filter(|(k, _)| k == "error").count();
    assert!(functions >= 6 && errors >= 3, "parsed {entries:?}");
    for variant in ["erc721", "erc721-solady"] {
        let src = read(&root().join(variant).join("files/src/Token.sol"));
        for (kind, n) in &entries {
            let found = match kind.as_str() {
                "function" => {
                    src.contains(&format!("function {n}("))
                        || src.contains(&format!("public constant {n} "))
                        || src.contains(&format!("public {n};"))
                        // Inherited from the ERC-721 base in both libraries.
                        || n == "balanceOf"
                }
                "error" => src.contains(&format!("error {n}(")),
                other => panic!("unexpected ABI entry type {other}"),
            };
            assert!(found, "{variant} lacks {kind} {n}");
        }
    }
}

/// Every identifier a template imports is reserved, so a collection name can never
/// shadow it (`contract Ownable is ... Ownable` would not compile, and a silent
/// shadow would be worse).
#[test]
fn every_imported_symbol_is_a_reserved_identifier() {
    let mut files = Vec::new();
    walk(&root(), &mut files);
    let mut seen = 0;
    for f in files
        .iter()
        .filter(|f| f.extension().is_some_and(|e| e == "sol"))
    {
        for line in read(f).lines() {
            let t = line.trim();
            let Some(rest) = t.strip_prefix("import {") else {
                continue;
            };
            let Some(end) = rest.find('}') else { continue };
            for sym in rest[..end].split(',') {
                let sym = sym.trim();
                if sym.starts_with("{{ct:") {
                    continue;
                }
                seen += 1;
                assert!(
                    citrate_templates::params::RESERVED_IDENTIFIERS.contains(&sym),
                    "{sym} (imported in {}) is not reserved",
                    f.display()
                );
            }
        }
    }
    assert!(seen > 10, "only {seen} imports scanned");
}

/// Every contract or interface a template declares under a fixed name is reserved
/// too (a helper like `ReentrantMinter` must not be shadowed by the collection).
#[test]
fn every_fixed_declaration_is_a_reserved_identifier() {
    let mut files = Vec::new();
    walk(&root(), &mut files);
    let mut seen = 0;
    for f in files
        .iter()
        .filter(|f| f.extension().is_some_and(|e| e == "sol"))
    {
        for line in read(f).lines() {
            let t = line.trim_start();
            let rest = ["abstract contract ", "contract ", "interface ", "library "]
                .iter()
                .find_map(|kw| t.strip_prefix(kw));
            let Some(rest) = rest else { continue };
            let decl: String = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            if decl.is_empty() || rest.starts_with("{{ct:") {
                continue;
            }
            seen += 1;
            assert!(
                citrate_templates::params::RESERVED_IDENTIFIERS.contains(&decl.as_str()),
                "{decl} (declared in {}) is not reserved",
                f.display()
            );
        }
    }
    assert!(seen >= 2, "only {seen} fixed declarations scanned");
}

#[test]
fn the_dependency_lock_pins_full_commit_shas() {
    let lock: serde_json::Value = match serde_json::from_str(&read(&root().join("deps.lock.json")))
    {
        Ok(v) => v,
        Err(e) => panic!("{e}"),
    };
    let deps = match lock["deps"].as_object() {
        Some(d) => d,
        None => panic!("deps.lock.json has no deps object"),
    };
    for name in ["openzeppelin-contracts", "solady", "forge-std"] {
        let d = &deps[name];
        let sha = d["commit"].as_str().unwrap_or("");
        assert_eq!(sha.len(), 40, "{name}");
        assert!(
            sha.bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()),
            "{name}"
        );
        assert!(
            d["tag"].as_str().is_some_and(|t| t.starts_with('v')),
            "{name}"
        );
        assert!(
            d["url"]
                .as_str()
                .is_some_and(|u| u.starts_with("https://github.com/")),
            "{name}"
        );
    }
    assert_eq!(lock["solc"].as_str(), Some("0.8.36"));
}

mod fixtures {
    use super::*;

    /// A throwaway template root with one template whose body is `body`.
    fn root_with(tag: &str, body: &str) -> Scratch {
        let s = Scratch::new(tag);
        let t = s.path().join("t1");
        let made = fs::create_dir_all(t.join("files"))
            .and_then(|_| {
                fs::write(
                    t.join("template.json"),
                    r#"{"id":"t1","kind":"contract","title":"T","description":"d","params":{"name":{}}}"#,
                )
            })
            .and_then(|_| fs::write(t.join("files/a.txt"), body))
            .and_then(|_| fs::copy(root().join("medusa-budgets.json"), s.path().join("medusa-budgets.json")))
            .and_then(|_| fs::copy(root().join("deps.lock.json"), s.path().join("deps.lock.json")));
        if let Err(e) = made {
            panic!("{e}");
        }
        s
    }

    fn render(r: &Scratch, out: &Scratch) -> Result<(), RenderError> {
        let s = TemplateSet::open(r.path())?;
        s.render("t1", &params(&[("name", "Lemon")]), Tier::T0, out.path())
            .map(|_| ())
    }

    #[test]
    fn an_unknown_placeholder_is_a_template_error() {
        let r = root_with("fx-unknown", "hi {{ct:owner}}");
        let out = Scratch::new("fx-unknown-out");
        assert!(matches!(
            render(&r, &out),
            Err(RenderError::Template { .. })
        ));
        assert!(!out.path().exists());
    }

    #[test]
    fn an_unterminated_placeholder_is_a_template_error() {
        let r = root_with("fx-unterminated", "hi {{ct:name");
        let out = Scratch::new("fx-unterminated-out");
        assert!(matches!(
            render(&r, &out),
            Err(RenderError::Template { .. })
        ));
    }

    #[test]
    fn jsx_double_braces_pass_through() {
        let r = root_with(
            "fx-jsx",
            "<div style={{ color: \"red\" }}>{{ct:name}}</div>",
        );
        let out = Scratch::new("fx-jsx-out");
        if let Err(e) = render(&r, &out) {
            panic!("{e}");
        }
        assert_eq!(
            read(&out.path().join("a.txt")),
            "<div style={{ color: \"red\" }}>Lemon</div>"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_inside_a_template_is_refused() {
        let r = root_with("fx-link", "x");
        if let Err(e) = std::os::unix::fs::symlink("/etc/hosts", r.path().join("t1/files/link")) {
            panic!("{e}");
        }
        let out = Scratch::new("fx-link-out");
        assert!(matches!(
            render(&r, &out),
            Err(RenderError::Template { .. })
        ));
        assert!(!out.path().exists());
    }

    #[test]
    fn a_manifest_with_an_unknown_param_is_refused() {
        let s = Scratch::new("fx-manifest");
        let t = s.path().join("t1");
        let made = fs::create_dir_all(t.join("files"))
            .and_then(|_| {
                fs::write(
                    t.join("template.json"),
                    r#"{"id":"t1","kind":"contract","title":"T","description":"d","params":{"script":{}}}"#,
                )
            })
            .and_then(|_| fs::copy(root().join("medusa-budgets.json"), s.path().join("medusa-budgets.json")))
            .and_then(|_| fs::copy(root().join("deps.lock.json"), s.path().join("deps.lock.json")));
        if let Err(e) = made {
            panic!("{e}");
        }
        assert!(TemplateSet::open(s.path()).is_err());
    }
}
