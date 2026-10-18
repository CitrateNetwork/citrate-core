use super::*;

fn none_set(_: &str) -> bool {
    false
}

#[test]
fn nothing_set_on_nvidia_sets_both() {
    assert_eq!(
        decide(none_set, true),
        vec![(DMABUF_RENDERER, "1"), (COMPOSITING_MODE, "1")]
    );
}

#[test]
fn nothing_set_without_nvidia_still_sets_both() {
    // #237 shipped both defaults for every Linux system; dropping compositing off NVIDIA
    // would regress the GPUs where that default was the fix.
    assert_eq!(
        decide(none_set, false),
        vec![(DMABUF_RENDERER, "1"), (COMPOSITING_MODE, "1")]
    );
}

#[test]
fn a_value_the_member_set_is_never_overridden() {
    for nvidia in [true, false] {
        assert_eq!(
            decide(|k| k == DMABUF_RENDERER, nvidia),
            vec![(COMPOSITING_MODE, "1")]
        );
        assert_eq!(
            decide(|k| k == COMPOSITING_MODE, nvidia),
            vec![(DMABUF_RENDERER, "1")]
        );
        assert!(decide(|_| true, nvidia).is_empty());
    }
}

fn scratch_root(tag: &str) -> std::path::PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let dir = std::env::temp_dir().join(format!(
        "citrate-linux-webkit-{tag}-{}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("scratch root");
    dir
}

#[test]
fn nvidia_is_detected_from_either_driver_file() {
    let none = scratch_root("none");
    assert!(!nvidia_present_under(&none));

    let module = scratch_root("module");
    std::fs::create_dir_all(module.join("sys/module/nvidia")).expect("mkdir module");
    assert!(nvidia_present_under(&module));

    let proc_root = scratch_root("proc");
    std::fs::create_dir_all(proc_root.join("proc/driver/nvidia")).expect("mkdir proc");
    std::fs::write(proc_root.join("proc/driver/nvidia/version"), "NVRM 580.126").expect("write");
    assert!(nvidia_present_under(&proc_root));

    for d in [none, module, proc_root] {
        let _ = std::fs::remove_dir_all(d);
    }
}

#[test]
fn the_log_line_names_the_gpu_and_each_override() {
    let line = summary(&decide(none_set, true), true);
    assert!(
        line.starts_with("[linux-webkit] NVIDIA driver present; applied "),
        "{line}"
    );
    assert!(line.contains("WEBKIT_DISABLE_DMABUF_RENDERER=1"), "{line}");
    assert!(line.contains("WEBKIT_DISABLE_COMPOSITING_MODE=1"), "{line}");

    let line = summary(&[], false);
    assert!(line.contains("no NVIDIA driver"), "{line}");
    assert!(line.contains("already set by the environment"), "{line}");
}

#[test]
fn run_applies_the_defaults_before_the_builder() {
    let lib = include_str!("lib.rs");
    let apply = lib
        .find("linux_webkit_env::apply()")
        .expect("run() calls apply");
    let builder = lib.find("tauri::Builder::default()").expect("builder");
    let run = lib.find("pub fn run()").expect("run");
    assert!(
        run < apply && apply < builder,
        "apply() runs first in run(), before the builder"
    );
}

/// The .deb must keep the identity every earlier package had (0.1.0 onward): package
/// `citrate-core`, `/usr/bin/citrate-core`, `/usr/share/applications/Citrate Core.desktop`, so
/// `apt install` upgrades it and replaces the old launcher in place. tauri-bundler derives
/// `Package:` from kebab-case(productName), the desktop file name from productName and the binary
/// from the main binary (`mainBinaryName`, else the Cargo package name).
#[test]
fn the_deb_keeps_the_package_binary_and_launcher_names() {
    let conf: serde_json::Value =
        serde_json::from_str(include_str!("../tauri.conf.json")).expect("tauri.conf.json");
    assert_eq!(conf["productName"], "Citrate Core");
    assert!(
        conf.get("mainBinaryName").is_none(),
        "mainBinaryName would rename /usr/bin/citrate-core"
    );
    let linux: serde_json::Value =
        serde_json::from_str(include_str!("../tauri.bundle-linux.conf.json")).expect("linux conf");
    assert!(
        linux.get("productName").is_none(),
        "the Linux overlay must not rename the package"
    );
    assert!(
        linux.get("mainBinaryName").is_none(),
        "the Linux overlay must not rename the binary"
    );
    let cargo = include_str!("../Cargo.toml");
    assert!(
        cargo
            .lines()
            .any(|l| l.trim() == r#"name = "citrate-core""#),
        "the Cargo package (main binary) stays citrate-core"
    );
}
