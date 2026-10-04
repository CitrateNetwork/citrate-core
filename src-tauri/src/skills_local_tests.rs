//! Tests for `skills_local.rs`: the member's skills are `SKILL.md` skills in the strict loader
//! format (HUP-S3.2 US-3.2 AC2), older flat files convert once, PBA-L7b-002 still holds.

use super::*;

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "citrate-skills-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// The exact text a save produces. The same bytes are a fixture in citrate-agent-runtime
/// (`agent-loop/tests/skills_tests.rs`, `core_written_skill_md_parses_strictly`), where the
/// sidecar's strict loader must accept them.
const MORNING_CHECK_MD: &str = "---\nname: morning-check\ndescription: \"Read node status, then the \\\"staking\\\" status.\"\nmetadata:\n  display-name: \"Morning check\"\n  origin: citrate-core-skill-write\n---\n\n1. node_status\n2. staking_status\n";

#[test]
fn slugify_is_safe_stable_and_a_valid_skill_name() {
    assert_eq!(slugify("Weekly Staking Report!"), "weekly-staking-report");
    assert_eq!(slugify("  ../etc/passwd  "), "etc-passwd");
    assert_eq!(slugify(""), "skill");
    assert_eq!(slugify("***"), "skill");
    let long = slugify(&"ab ".repeat(40));
    assert!(long.len() <= 64, "{long}");
    assert!(!long.ends_with('-') && !long.starts_with('-') && !long.contains("--"));
}

#[test]
fn a_saved_skill_is_a_strict_skill_md_in_its_own_folder() {
    let dir = scratch("strict");
    let s = write_skill_file(
        &dir,
        "Morning check",
        "Read node status, then the \"staking\" status.",
        "1. node_status\n2. staking_status",
        false,
    )
    .unwrap();
    assert_eq!(s.slug, "morning-check");
    assert_eq!(s.name, "Morning check");
    let text = fs::read_to_string(dir.join("morning-check/SKILL.md")).unwrap();
    assert_eq!(text, MORNING_CHECK_MD);
    assert!(
        !dir.join("morning-check.md").exists(),
        "no flat file any more"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn multi_line_names_and_descriptions_stay_on_one_frontmatter_line() {
    let dir = scratch("oneline");
    write_skill_file(&dir, "Two\nlines", "a\nb\tc", "body", false).unwrap();
    let text = fs::read_to_string(dir.join("two-lines/SKILL.md")).unwrap();
    let fm: Vec<&str> = text.split("---\n").nth(1).unwrap().lines().collect();
    assert_eq!(fm[1], "description: \"a b c\"");
    assert_eq!(fm[3], "  display-name: \"Two lines\"");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn an_empty_description_becomes_an_honest_one_from_the_instructions() {
    // The loader refuses a skill without a description.
    assert_eq!(
        description_for("", "\n\nCheck the node.\nThen stake."),
        "A skill the member saved: Check the node."
    );
    assert_eq!(description_for("  ", ""), "A skill the member saved.");
    assert_eq!(description_for("Mine.", "x"), "Mine.");
}

#[test]
fn list_and_read_round_trip_the_display_name_description_and_body() {
    let dir = scratch("roundtrip");
    write_skill_file(
        &dir,
        "Morning check",
        "Reads \"status\" \\ fast",
        "Step 1. do it",
        false,
    )
    .unwrap();
    let list = list_skills(&dir);
    assert_eq!(
        list,
        vec![LocalSkill {
            name: "Morning check".into(),
            description: "Reads \"status\" \\ fast".into(),
            slug: "morning-check".into(),
        }]
    );
    assert_eq!(read_body(&dir, "Morning check").unwrap(), "Step 1. do it\n");
    assert_eq!(read_body(&dir, "morning-check").unwrap(), "Step 1. do it\n");
    assert!(read_body(&dir, "nope").is_err());
    let _ = fs::remove_dir_all(&dir);
}

/// PBA-L7b-002: a create-only write never replaces an existing skill (the persistence vector
/// for an injected instruction); an explicit, member-approved overwrite does.
#[test]
fn pba_l7b_002_existing_skill_is_not_silently_overwritten() {
    let dir = scratch("l7b002");
    write_skill_file(&dir, "Daily", "d", "original steps", false).expect("first write");
    let err = write_skill_file(&dir, "Daily", "d", "INJECTED steps", false)
        .expect_err("a second create-only write must be refused");
    assert!(err.starts_with(SKILL_EXISTS_PREFIX), "got {err}");
    let body = fs::read_to_string(dir.join("daily/SKILL.md")).unwrap();
    assert!(body.contains("original steps") && !body.contains("INJECTED"));
    write_skill_file(&dir, "Daily", "d", "approved replacement", true).expect("approved overwrite");
    let body = fs::read_to_string(dir.join("daily/SKILL.md")).unwrap();
    assert!(body.contains("approved replacement"));
    let _ = fs::remove_dir_all(&dir);
}

/// Mutation hardening (cargo-mutants on write_skill_file): each size bound is exact.
#[test]
fn pba_l7b_002_write_skill_file_size_bounds_are_exact() {
    let dir = scratch("bounds");
    let n = |len: usize, c: char| std::iter::repeat_n(c, len).collect::<String>();
    assert!(write_skill_file(&dir, &n(MAX_NAME, 'a'), "d", "i", false).is_ok());
    assert!(write_skill_file(&dir, &n(MAX_NAME + 1, 'b'), "d", "i", false).is_err());
    assert!(write_skill_file(&dir, "c", &n(MAX_DESC, 'd'), "i", false).is_ok());
    assert!(write_skill_file(&dir, "e", &n(MAX_DESC + 1, 'd'), "i", false).is_err());
    assert!(write_skill_file(&dir, "f", "d", &n(MAX_BODY, 'x'), false).is_ok());
    assert!(write_skill_file(&dir, "g", "d", &n(MAX_BODY + 1, 'x'), false).is_err());
    // The largest allowed skill still fits the loader's 64 KiB SKILL.md cap.
    let big = fs::metadata(dir.join("f/SKILL.md")).unwrap().len();
    assert!(big <= 64 * 1024, "{big}");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_legacy_flat_skill_converts_once_and_the_old_file_is_kept() {
    let dir = scratch("migrate");
    fs::write(
        dir.join("weekly-report.md"),
        "---\nname: Weekly report\ndescription: does a thing\n---\n\nStep 1. do it\n",
    )
    .unwrap();
    let r = migrate_legacy(&dir);
    assert_eq!(r.converted, vec!["weekly-report".to_string()]);
    assert!(r.failed.is_empty());
    assert!(dir.join("weekly-report.md.migrated").is_file());
    assert!(!dir.join("weekly-report.md").exists());
    let text = fs::read_to_string(dir.join("weekly-report/SKILL.md")).unwrap();
    assert!(text.starts_with("---\nname: weekly-report\ndescription: \"does a thing\"\n"));
    assert_eq!(read_body(&dir, "Weekly report").unwrap(), "Step 1. do it\n");
    // Idempotent: a second pass finds nothing to do.
    assert_eq!(migrate_legacy(&dir), MigrationReport::default());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_legacy_skill_that_collides_is_reported_and_left_in_place() {
    let dir = scratch("collide");
    write_skill_file(&dir, "Daily", "new", "new steps", false).unwrap();
    fs::write(
        dir.join("daily.md"),
        "---\nname: Daily\ndescription: old\n---\nold steps\n",
    )
    .unwrap();
    let r = migrate_legacy(&dir);
    assert!(r.converted.is_empty());
    assert_eq!(r.failed.len(), 1);
    assert_eq!(r.failed[0].file, "daily.md");
    assert!(
        r.failed[0].reason.starts_with(SKILL_EXISTS_PREFIX),
        "{:?}",
        r.failed
    );
    assert!(dir.join("daily.md").is_file(), "never lost");
    assert_eq!(read_body(&dir, "Daily").unwrap(), "new steps\n");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_legacy_skill_without_frontmatter_converts_with_its_file_name() {
    let dir = scratch("nofm");
    fs::write(dir.join("plain.md"), "just text\n").unwrap();
    let r = migrate_legacy(&dir);
    assert_eq!(r.converted, vec!["plain".to_string()]);
    let list = list_skills(&dir);
    assert_eq!(list[0].description, "A skill the member saved: just text");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn delete_removes_only_the_skill_folder_and_is_idempotent() {
    let dir = scratch("delete");
    write_skill_file(&dir, "Gone", "d", "x", false).unwrap();
    write_skill_file(&dir, "Kept", "d", "y", false).unwrap();
    delete_skill(&dir, "Gone").unwrap();
    delete_skill(&dir, "Gone").unwrap();
    assert!(!dir.join("gone").exists());
    assert_eq!(list_skills(&dir).len(), 1);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn the_authored_folder_is_under_app_local_data() {
    assert_eq!(
        authored_skills_dir(Path::new("/data")),
        PathBuf::from("/data/agent-skills")
    );
}
