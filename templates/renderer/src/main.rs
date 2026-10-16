//! `citrate-templates`: list or render the HUP templates.
//!
//! ```text
//! citrate-templates list   --root <templates dir>
//! citrate-templates render --root <templates dir> --template <id> --tier <T0|T1|T2>
//!                          --out <empty dir> [--param key=value]...
//! ```
//!
//! Prints JSON on stdout; errors go to stderr with exit code 2.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::ExitCode;

use citrate_templates::{TemplateSet, Tier};

const USAGE: &str = "usage:\n  citrate-templates list --root <dir>\n  citrate-templates render --root <dir> --template <id> --tier <T0|T1|T2> --out <dir> [--param key=value]...";

fn run(args: &[String]) -> Result<String, String> {
    let Some(cmd) = args.first() else {
        return Err(USAGE.to_string());
    };
    let mut root: Option<PathBuf> = None;
    let mut template: Option<String> = None;
    let mut tier: Option<Tier> = None;
    let mut out: Option<PathBuf> = None;
    let mut params: BTreeMap<String, String> = BTreeMap::new();
    let mut i = 1;
    while i < args.len() {
        let flag = args[i].as_str();
        let value = args
            .get(i + 1)
            .ok_or_else(|| format!("{flag} needs a value\n{USAGE}"))?;
        match flag {
            "--root" => root = Some(PathBuf::from(value)),
            "--template" => template = Some(value.clone()),
            "--tier" => {
                tier = Some(
                    Tier::parse(value)
                        .ok_or_else(|| format!("bad tier {value:?} (expected T0, T1 or T2)"))?,
                )
            }
            "--out" => out = Some(PathBuf::from(value)),
            "--param" => {
                let (k, v) = value
                    .split_once('=')
                    .ok_or_else(|| format!("--param needs key=value, got {value:?}"))?;
                if params.insert(k.to_string(), v.to_string()).is_some() {
                    return Err(format!("--param {k} given twice"));
                }
            }
            other => return Err(format!("unknown flag {other:?}\n{USAGE}")),
        }
        i += 2;
    }
    let root = root.ok_or_else(|| format!("--root is required\n{USAGE}"))?;
    let set = TemplateSet::open(&root).map_err(|e| e.to_string())?;
    match cmd.as_str() {
        "list" => {
            let list: Vec<_> = set.ids().filter_map(|id| set.manifest(id)).collect();
            serde_json::to_string_pretty(&list).map_err(|e| e.to_string())
        }
        "render" => {
            let template = template.ok_or_else(|| format!("--template is required\n{USAGE}"))?;
            let tier = tier.ok_or_else(|| format!("--tier is required\n{USAGE}"))?;
            let out = out.ok_or_else(|| format!("--out is required\n{USAGE}"))?;
            let report = set
                .render(&template, &params, tier, &out)
                .map_err(|e| e.to_string())?;
            serde_json::to_string_pretty(&report).map_err(|e| e.to_string())
        }
        other => Err(format!("unknown command {other:?}\n{USAGE}")),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(json) => {
            println!("{json}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("citrate-templates: {e}");
            ExitCode::from(2)
        }
    }
}
