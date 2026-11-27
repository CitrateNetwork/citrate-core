//! The app's commands, registered per lane (L0-WP03, planset v060 L0_INTEGRATION_SEAMS 3.1).
//!
//! Each `l<N>.rs` defines one macro, `v60_commands_l<N>`, that appends that lane's command paths
//! to an accumulator and hands it to `v60_commands_next`, with the names of the macros still to
//! run. This file fixes that order (l0 to l9, then finish), and `v60_commands_finish` expands to
//! the one call of tauri's `generate_handler` macro with every lane's paths. lib.rs registers them
//! with `.invoke_handler(crate::v60_all_commands!())`. So two lanes that add commands on the same
//! day edit two different files and never conflict.
//!
//! A new command goes in its lane's list and in that lane's ACL, `permissions/l<N>-commands.toml`.
//! Those two lane-owned files are the reviewed record of the addition: the L0 commands fixture
//! (`tests/fixtures/seams/commands.a826e441.txt`) is not edited, and `seams_tests` still names any
//! recorded command that is dropped or re-pointed. Only `capabilities/default.json` (the main
//! window) holds the ten lane permissions; at the split they allowed exactly the commands the
//! single HUP-S5.4 `main-window-commands` permission did.
//!
//! Macro notes:
//! - The paths travel as `tt`, never as `$x:path`: a `path` fragment reaches a proc macro as one
//!   opaque group, and tauri's `generate_handler` takes each path apart to name its wrapper.
//! - The paths are written exactly as they were in lib.rs (`comms::comms_send`, no `crate::`
//!   prefix; `crate::` inside a `#[macro_export]` macro is clippy's `crate_in_macro_def`). A
//!   `macro_rules!` path resolves where the macro is invoked, so `v60_all_commands!()` is invoked in
//!   one place only, `run()` in lib.rs at the crate root (`seams_tests` checks it).
//! - No macro is called through a metavariable (`$next!`): the K6 source scan
//!   (`kit/src/wallet_tests.rs`, `k6_module_refs`) refuses that form because `$next` could be
//!   `include`. `v60_commands_next` matches each macro name literally instead.
//! - `#[macro_export]` puts every macro at the crate root; each name starts with `v60_commands_`.

mod l0;
mod l1;
mod l2;
mod l3;
mod l4;
mod l5;
mod l6;
mod l7;
mod l8;
mod l9;

/// Every lane's commands, as the one invoke handler the builder takes.
#[macro_export]
macro_rules! v60_all_commands {
    () => {
        $crate::v60_commands_l0!(
            v60_commands_l1 v60_commands_l2 v60_commands_l3 v60_commands_l4 v60_commands_l5
            v60_commands_l6 v60_commands_l7 v60_commands_l8 v60_commands_l9 v60_commands_finish []
        )
    };
}

/// The hand-off between lanes: the first name says which macro runs next. Each arm names its macro
/// literally (see the module notes); the accumulator is passed on unchanged.
#[macro_export]
#[doc(hidden)]
macro_rules! v60_commands_next {
    (v60_commands_l1 $($rest:ident)* [$($a:tt)*]) => { $crate::v60_commands_l1!($($rest)* [$($a)*]) };
    (v60_commands_l2 $($rest:ident)* [$($a:tt)*]) => { $crate::v60_commands_l2!($($rest)* [$($a)*]) };
    (v60_commands_l3 $($rest:ident)* [$($a:tt)*]) => { $crate::v60_commands_l3!($($rest)* [$($a)*]) };
    (v60_commands_l4 $($rest:ident)* [$($a:tt)*]) => { $crate::v60_commands_l4!($($rest)* [$($a)*]) };
    (v60_commands_l5 $($rest:ident)* [$($a:tt)*]) => { $crate::v60_commands_l5!($($rest)* [$($a)*]) };
    (v60_commands_l6 $($rest:ident)* [$($a:tt)*]) => { $crate::v60_commands_l6!($($rest)* [$($a)*]) };
    (v60_commands_l7 $($rest:ident)* [$($a:tt)*]) => { $crate::v60_commands_l7!($($rest)* [$($a)*]) };
    (v60_commands_l8 $($rest:ident)* [$($a:tt)*]) => { $crate::v60_commands_l8!($($rest)* [$($a)*]) };
    (v60_commands_l9 $($rest:ident)* [$($a:tt)*]) => { $crate::v60_commands_l9!($($rest)* [$($a)*]) };
    (v60_commands_finish [$($a:tt)*]) => { $crate::v60_commands_finish!([$($a)*]) };
}

/// The end of the chain: the accumulated paths of l0 to l9, in that order, and nothing else.
#[macro_export]
#[doc(hidden)]
macro_rules! v60_commands_finish {
    ([$($cmd:tt)*]) => {
        ::tauri::generate_handler![$($cmd)*]
    };
}

/// The ten lanes, in chain order.
#[cfg(test)]
pub(crate) const LANES: [&str; 10] = ["l0", "l1", "l2", "l3", "l4", "l5", "l6", "l7", "l8", "l9"];

/// Each lane's list, in chain order.
#[cfg(test)]
const LANE_SOURCES: [&str; 10] = [
    include_str!("l0.rs"),
    include_str!("l1.rs"),
    include_str!("l2.rs"),
    include_str!("l3.rs"),
    include_str!("l4.rs"),
    include_str!("l5.rs"),
    include_str!("l6.rs"),
    include_str!("l7.rs"),
    include_str!("l8.rs"),
    include_str!("l9.rs"),
];

/// Each lane's ACL, in chain order.
#[cfg(test)]
const LANE_ACLS: [&str; 10] = [
    include_str!("../../permissions/l0-commands.toml"),
    include_str!("../../permissions/l1-commands.toml"),
    include_str!("../../permissions/l2-commands.toml"),
    include_str!("../../permissions/l3-commands.toml"),
    include_str!("../../permissions/l4-commands.toml"),
    include_str!("../../permissions/l5-commands.toml"),
    include_str!("../../permissions/l6-commands.toml"),
    include_str!("../../permissions/l7-commands.toml"),
    include_str!("../../permissions/l8-commands.toml"),
    include_str!("../../permissions/l9-commands.toml"),
];

/// Where a lane's own paths start inside its macro (after the accumulator).
// Assembled from parts so this file holds no list opening of its own (`seams_tests` reads it).
#[cfg(test)]
const LIST_OPEN: &str = concat!("[$($", "acc)*");

/// What every structural test that read lib.rs for the command list reads now: lib.rs,
/// this file and every lane file, concatenated in that order. A registration can only be written
/// in one of them, so a needle that matched (or was absent from) lib.rs before still does here,
/// and an offset found in the lib.rs part is the same offset as in lib.rs.
#[cfg(test)]
pub(crate) fn registry_source() -> String {
    let mut out = String::from(include_str!("../lib.rs"));
    out.push_str(include_str!("mod.rs"));
    for src in LANE_SOURCES {
        out.push_str(src);
    }
    out
}

/// Every lane's ACL file, concatenated in chain order: what the tests that read
/// `permissions/main-window.toml` read now.
#[cfg(test)]
pub(crate) fn acl_source() -> String {
    LANE_ACLS.concat()
}

/// `(permission identifier, ACL file text)` per lane, in chain order.
#[cfg(test)]
pub(crate) fn lane_acls() -> Vec<(String, &'static str)> {
    LANES
        .iter()
        .zip(LANE_ACLS)
        .map(|(lane, toml)| (format!("{lane}-commands"), toml))
        .collect()
}

/// `(lane, list file text)` per lane, in chain order.
#[cfg(test)]
pub(crate) fn lane_sources() -> Vec<(&'static str, &'static str)> {
    LANES.into_iter().zip(LANE_SOURCES).collect()
}

/// The command paths one lane file appends, in file order (comments dropped).
#[cfg(test)]
pub(crate) fn lane_paths(src: &str) -> Vec<String> {
    let Some(at) = src.find(LIST_OPEN) else {
        return Vec::new();
    };
    let start = at + LIST_OPEN.len();
    let Some(len) = src[start..].find("])") else {
        return Vec::new();
    };
    src[start..start + len]
        .lines()
        .map(|l| l.split("//").next().unwrap_or_default())
        .flat_map(|l| l.split(','))
        .map(str::trim)
        .filter(|s| s.contains("::"))
        .map(str::to_string)
        .collect()
}

/// Every registered command path, l0 to l9, each lane in its file order.
#[cfg(test)]
pub(crate) fn registered_paths() -> Vec<String> {
    LANE_SOURCES.into_iter().flat_map(lane_paths).collect()
}

/// Every registered command name (the last path segment): what the webview invokes.
#[cfg(test)]
pub(crate) fn registered_names() -> std::collections::BTreeSet<String> {
    registered_paths()
        .iter()
        .filter_map(|p| p.rsplit("::").next().map(str::to_string))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_finish_macro_adds_no_command_of_its_own() {
        let src = include_str!("mod.rs");
        let flat: String = src.split_whitespace().collect::<Vec<_>>().join(" ");
        // Assembled from parts so this test's own text is not a second call.
        let body =
            "([$($cmd:tt)*]) => { ::tauri::".to_string() + "generate_handler" + "![$($cmd)*] };";
        assert_eq!(
            flat.matches(&body).count(),
            1,
            "v60_commands_finish passes the accumulated paths through unchanged"
        );
    }

    #[test]
    fn each_hand_off_runs_the_macro_it_names() {
        let src = include_str!("mod.rs");
        let flat: String = src.split_whitespace().collect::<Vec<_>>().join(" ");
        for lane in &LANES[1..] {
            let arm = format!(
                "(v60_commands_{lane} $($rest:ident)* [$($a:tt)*]) {arrow} {{ \
                 $crate::v60_commands_{lane}!($($rest)* [$($a)*]) }};",
                arrow = "=>"
            );
            assert_eq!(flat.matches(&arm).count(), 1, "{arm}");
        }
        // The needles below are assembled from parts so this test's own text does not match.
        let fin = format!(
            "(v60_commands_{} [$($a:tt)*]) => {{ $crate::v60_commands_{}!([$($a)*]) }};",
            "finish", "finish"
        );
        assert_eq!(flat.matches(&fin).count(), 1, "{fin}");
        // l0 is the entry, never a hand-off target: nine lane arms, then the finish arm.
        assert!(!flat.contains(&format!("(v60_commands_{} ", "l0")));
        let lane_arm = format!("$($rest:ident)* [$($a:tt)*]) {}", "=>");
        assert_eq!(flat.matches(&lane_arm).count(), 9);
    }

    #[test]
    fn registry_source_is_lib_rs_then_every_lane_file() {
        let src = registry_source();
        let lib = include_str!("../lib.rs");
        assert!(src.starts_with(lib), "lib.rs comes first, unchanged");
        let mut at = lib.len();
        for part in std::iter::once(include_str!("mod.rs")).chain(LANE_SOURCES) {
            assert_eq!(&src[at..at + part.len()], part);
            at += part.len();
        }
        assert_eq!(at, src.len(), "nothing else is read");
        assert_eq!(acl_source(), LANE_ACLS.concat());
    }

    #[test]
    fn the_helpers_see_a_registration_added_to_any_lane() {
        // Mutation: one more line in each lane's list is seen by the paths, the names and the
        // source every structural test reads.
        for (i, lane) in LANES.iter().enumerate() {
            let line = format!("            wp03_probe::probe_{lane},\n");
            let src = LANE_SOURCES[i].replacen(LIST_OPEN, &format!("{LIST_OPEN}\n{line}"), 1);
            let paths = lane_paths(&src);
            assert_eq!(paths.first(), Some(&format!("wp03_probe::probe_{lane}")));
            assert_eq!(paths.len(), lane_paths(LANE_SOURCES[i]).len() + 1);
        }
        let names = registered_names();
        assert_eq!(names.len(), registered_paths().len(), "no name twice");
        assert!(names.contains("config_read") && names.contains("node_mcp_decide"));
    }
}
