//! `sond poke <id>` — continue an existing investigation.
//!
//! Contract established here:
//!
//! - the log is found by the `R<id>-` prefix of its filename in `./sond/`
//! - the ID may be typed as `R042`, `r42`, `42`, ...
//! - Sond appends `\n---\n\n## <YYYY-MM-DD HH:MM>\n\n` and never rewrites a
//!   byte of what was there (a missing final newline is added first)
//! - if the log already ends in an empty dated section, nothing is appended
//!   and that section is reopened
//! - stdout is the relative path of the log, one line
//! - no editor is opened unless asked for: with no `$VISUAL` or `$EDITOR`
//!   set, `sond poke` still just appends the section and succeeds
//! - with `-e` / `--edit`, the editor then opens the log at the new section
//! - unknown IDs, duplicate IDs and invalid IDs are errors that touch nothing

#![cfg(unix)]

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;

use common::{NOW, Project, fixture, sh_editor};
use predicates::prelude::*;
use sond::clock::parse_timestamp;
use sond::log::{poke_log, trailing_empty_section_line};

const R003: &str = "R003-2026-09-20-adaptive-timestep.md";
const FIXTURES: [&str; 3] = [
    "R001-2026-09-18-boundary-condition.md",
    "R002-2026-09-19-fourier-features.md",
    R003,
];

fn project() -> Project {
    Project::with_fixture_logs(&FIXTURES)
}

fn original(name: &str) -> String {
    fs::read_to_string(fixture(&format!("logs/{name}"))).unwrap()
}

fn section(timestamp: &str) -> String {
    format!("\n---\n\n## {timestamp}\n\n")
}

/// Asserts no log in the project differs from its fixture.
fn assert_untouched(p: &Project) {
    assert_eq!(p.log_names(), FIXTURES);
    for name in FIXTURES {
        assert_eq!(p.read_log(name), original(name), "{name} was modified");
    }
}

// ---------------------------------------------------------------------------
// CLI contract
// ---------------------------------------------------------------------------

#[test]
fn help_lists_poke() {
    Project::empty()
        .sond()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("poke"));
}

#[test]
fn poke_has_help() {
    Project::empty()
        .sond()
        .args(["poke", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("<ID>"));
}

#[test]
fn poke_requires_an_id() {
    let p = project();
    p.sond().arg("poke").assert().failure();
    assert_untouched(&p);
}

#[test]
fn invalid_id_is_an_error_that_touches_nothing() {
    let p = project();
    p.sond()
        .args(["poke", "banana"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("banana"));
    assert_untouched(&p);
    assert!(p.editor_invocations().is_empty());
}

// ---------------------------------------------------------------------------
// finding the log
// ---------------------------------------------------------------------------

#[test]
fn every_human_spelling_of_the_id_finds_the_log() {
    for id in ["R003", "r003", "R3", "3", "003", "R0003", " R003 "] {
        let p = project();
        p.sond().args(["poke", id]).assert().success();
        assert_eq!(
            p.read_log(R003),
            original(R003) + &section(NOW),
            "for {id:?}"
        );
    }
}

#[test]
fn renamed_logs_are_found_by_their_id_prefix() {
    let p = Project::empty();
    p.add_log("R007-my-own-name.md", "# Renamed\n");
    p.sond().args(["poke", "R7"]).assert().success();
    assert_eq!(
        p.read_log("R007-my-own-name.md"),
        format!("# Renamed\n{}", section(NOW))
    );
}

#[test]
fn unknown_id_is_an_error_that_touches_nothing() {
    let p = project();
    p.sond()
        .args(["poke", "R999"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("R999").and(predicate::str::contains("panicked").not()));
    assert_untouched(&p);
    assert!(p.editor_invocations().is_empty());
}

#[test]
fn missing_logs_directory_is_an_error_and_is_not_created() {
    let p = Project::empty();
    p.sond()
        .args(["poke", "R001"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("R001"));
    assert!(!p.logs_dir().exists());
}

#[test]
fn the_id_inside_the_document_does_not_matter() {
    // The filename is the index. A log whose body claims another ID is not
    // found under that ID.
    let p = Project::empty();
    p.add_log("R003-2026-09-20-x.md", "# X\n\nID: R005\n");
    p.sond().args(["poke", "R005"]).assert().failure();
    assert_eq!(p.read_log("R003-2026-09-20-x.md"), "# X\n\nID: R005\n");
}

#[test]
fn duplicate_ids_are_an_error_naming_every_culprit() {
    let p = project();
    let dup = "R003-2026-09-22-copy-of-adaptive.md";
    p.add_log(dup, &original(R003));

    p.sond()
        .args(["poke", "R003"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(R003).and(predicate::str::contains(dup)));

    assert_eq!(p.read_log(R003), original(R003));
    assert_eq!(p.read_log(dup), original(R003));
    assert!(p.editor_invocations().is_empty());
}

#[test]
fn a_zero_padded_twin_is_still_a_duplicate() {
    // `R3-` and `R003-` both mean 3.
    let p = project();
    p.add_log("R3-short.md", "# short\n");
    p.sond().args(["poke", "3"]).assert().failure();
    assert_eq!(p.read_log(R003), original(R003));
}

// ---------------------------------------------------------------------------
// appending
// ---------------------------------------------------------------------------

#[test]
fn appends_a_dated_section_after_the_existing_bytes() {
    let p = project();
    p.sond().args(["poke", "R003"]).assert().success();
    assert_eq!(
        p.read_log(R003),
        format!(
            "\
# Adaptive timestep instability

Created: 2026-09-20 10:15
ID: R003

## Context

The simulation becomes unstable when the timestep is increased.

## Investigation

Initial experiments suggest the instability is related to the CFL condition.

---

## {NOW}

"
        )
    );
}

#[test]
fn does_not_create_a_file_or_touch_other_logs() {
    let p = project();
    p.sond().args(["poke", "R003"]).assert().success();
    assert_eq!(p.log_names(), FIXTURES);
    for name in &FIXTURES[..2] {
        assert_eq!(p.read_log(name), original(name));
    }
}

#[test]
fn a_missing_final_newline_is_added_before_the_section() {
    let p = Project::empty();
    p.add_log("R001-2026-09-18-x.md", "# X\n\nlast line without newline");
    p.sond().args(["poke", "R001"]).assert().success();
    assert_eq!(
        p.read_log("R001-2026-09-18-x.md"),
        format!("# X\n\nlast line without newline\n{}", section(NOW))
    );
}

#[test]
fn an_empty_file_gets_a_section() {
    let p = Project::empty();
    p.add_log("R001-2026-09-18-x.md", "");
    p.sond().args(["poke", "R001"]).assert().success();
    assert_eq!(p.read_log("R001-2026-09-18-x.md"), section(NOW));
}

#[test]
fn non_utf8_content_is_preserved_byte_for_byte() {
    let p = Project::empty();
    let path = p.logs_dir().join("R001-2026-09-18-x.md");
    fs::create_dir_all(p.logs_dir()).unwrap();
    let bytes = b"# Latin-1: \xe9t\xe9\n".to_vec();
    fs::write(&path, &bytes).unwrap();

    p.sond().args(["poke", "R001"]).assert().success();

    let mut expected = bytes;
    expected.extend_from_slice(section(NOW).as_bytes());
    assert_eq!(fs::read(&path).unwrap(), expected);
}

#[test]
fn poking_again_later_adds_another_section() {
    let p = project();
    p.sond()
        .env("FAKE_EDITOR_APPEND", "Halving dt fixes it.")
        .args(["poke", "-e", "R003"])
        .assert()
        .success();
    p.sond()
        .env("SOND_NOW", "2026-09-22 08:30")
        .env("FAKE_EDITOR_APPEND", "But only for the linear case.")
        .args(["poke", "-e", "R003"])
        .assert()
        .success();

    assert_eq!(
        p.read_log(R003),
        format!(
            "{}{}Halving dt fixes it.\n{}But only for the linear case.\n",
            original(R003),
            section(NOW),
            section("2026-09-22 08:30"),
        )
    );
}

#[test]
fn an_abandoned_empty_section_is_reopened_not_stacked() {
    let p = project();
    p.sond().args(["poke", "-e", "R003"]).assert().success();
    p.sond()
        .env("SOND_NOW", "2026-09-22 08:30")
        .args(["poke", "-e", "R003"])
        .assert()
        .success();

    assert_eq!(p.read_log(R003), original(R003) + &section(NOW));
    assert_eq!(p.editor_invocations().len(), 2);
}

#[test]
fn a_trailing_template_heading_is_not_mistaken_for_an_empty_poke() {
    // A fresh log from the default template ends in an empty `## Next steps`.
    let p = Project::empty();
    let fresh = "# T\n\nCreated: 2026-09-20 10:15\nID: R001\n\n## Next steps\n";
    p.add_log("R001-2026-09-20-t.md", fresh);
    p.sond().args(["poke", "R001"]).assert().success();
    assert_eq!(
        p.read_log("R001-2026-09-20-t.md"),
        format!("{fresh}{}", section(NOW))
    );
}

// ---------------------------------------------------------------------------
// output and editor
// ---------------------------------------------------------------------------

#[test]
fn does_not_open_an_editor_by_default() {
    let p = project();
    p.sond()
        .args(["poke", "R003"])
        .assert()
        .success()
        .stdout(format!("sond/{R003}\n"));
    assert_eq!(p.read_log(R003), original(R003) + &section(NOW));
    assert!(p.editor_invocations().is_empty());
}

#[test]
fn without_any_editor_configured_the_section_is_just_appended() {
    // The user's real setup: neither `$VISUAL` nor `$EDITOR` is set. Sond
    // must not fall back to an interactive program such as `vi`.
    let p = project();
    p.sond_without_editor()
        .args(["poke", "R003"])
        .assert()
        .success()
        .stdout(format!("sond/{R003}\n"));
    assert_eq!(p.read_log(R003), original(R003) + &section(NOW));

    assert!(p.editor_invocations().is_empty());
}

#[test]
fn edit_flag_prints_the_path_and_opens_the_same_log() {
    for flag in ["-e", "--edit"] {
        let p = project();
        p.sond()
            .args(["poke", flag, "R003"])
            .assert()
            .success()
            .stdout(format!("sond/{R003}\n"));
        assert_eq!(
            p.editor_invocations(),
            [[format!("sond/{R003}")]],
            "for {flag}"
        );
    }
}

#[test]
fn edit_flag_may_follow_the_id() {
    let p = project();
    p.sond().args(["poke", "R003", "-e"]).assert().success();
    assert_eq!(p.editor_invocations(), [[format!("sond/{R003}")]]);
}

#[test]
fn line_aware_editors_land_below_the_new_heading() {
    // The fake editor is run through `/bin/sh`, which Sond cannot give a line
    // number to; tests/fixtures/editor/code is the same editor named `code`.
    let p = project();
    let code = fixture("editor/code");
    p.sond()
        .env("EDITOR", format!("{} --wait", code.display()))
        .args(["poke", "-e", "R003"])
        .assert()
        .success();

    // The fixture is 12 lines; blank, `---`, blank puts the heading on line
    // 16, and the cursor goes on the line below it.
    assert_eq!(
        p.editor_invocations(),
        [["--wait", "--goto", &format!("sond/{R003}:17")]]
    );
}

#[test]
fn failing_editor_is_an_error_but_the_section_is_kept() {
    let p = project();
    p.sond()
        .env("EDITOR", sh_editor("editor/failing-editor.sh"))
        .args(["poke", "-e", "R003"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("editor"));
    assert_eq!(p.read_log(R003), original(R003) + &section(NOW));
}

// ---------------------------------------------------------------------------
// failures that must not corrupt anything
// ---------------------------------------------------------------------------

#[test]
fn invalid_clock_is_an_error_that_touches_nothing() {
    let p = project();
    p.sond()
        .env("SOND_NOW", "not a time")
        .args(["poke", "R003"])
        .assert()
        .failure();
    assert_untouched(&p);
}

#[test]
fn read_only_log_is_an_error_not_a_panic() {
    let p = project();
    let path = p.logs_dir().join(R003);
    fs::set_permissions(&path, fs::Permissions::from_mode(0o444)).unwrap();

    let assert = p.sond().args(["poke", "R003"]).assert();
    if fs::OpenOptions::new().append(true).open(&path).is_ok() {
        // Running as root: permissions are not enforced, nothing to test.
        return;
    }
    assert
        .failure()
        .stderr(predicate::str::contains(R003).and(predicate::str::contains("panicked").not()));
    assert_eq!(p.read_log(R003), original(R003));
    assert!(p.editor_invocations().is_empty());
}

// ---------------------------------------------------------------------------
// workflow
// ---------------------------------------------------------------------------

#[test]
fn new_then_poke_grows_one_investigation() {
    let p = Project::empty();
    p.sond()
        .env("SOND_NOW", "2026-09-20 10:15")
        .env("FAKE_EDITOR_APPEND", "dt > 0.01 blows up.")
        .args(["new", "-e", "Adaptive timestep instability"])
        .assert()
        .success();
    p.sond()
        .env("FAKE_EDITOR_APPEND", "CFL number exceeds 1 at that dt.")
        .args(["poke", "-e", "R001"])
        .assert()
        .success();

    let name = "R001-2026-09-20-adaptive-timestep-instability.md";
    assert_eq!(p.log_names(), [name]);
    let log = p.read_log(name);
    assert!(log.starts_with("# Adaptive timestep instability\n"));
    assert!(log.ends_with(&format!(
        "dt > 0.01 blows up.\n{}CFL number exceeds 1 at that dt.\n",
        section(NOW)
    )));
    assert_eq!(
        p.editor_invocations(),
        [[format!("sond/{name}")], [format!("sond/{name}")]]
    );
}

// ---------------------------------------------------------------------------
// poke_log: the line the editor is sent to
// ---------------------------------------------------------------------------

#[test]
fn poke_log_returns_the_heading_line_it_wrote() {
    let now = parse_timestamp(NOW).unwrap();
    for before in [
        "",
        "\n",
        "# T\n",
        "# T",
        "# T\n\nbody\n",
        "# T\n\nbody",
        "# T\n\n\n\n",
        "# T\r\n\r\nwindows\r\n",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("R001-x.md");
        fs::write(&path, before).unwrap();

        let line = poke_log(&path, now).unwrap();

        let after = fs::read_to_string(&path).unwrap();
        assert_eq!(
            after.lines().nth(line - 1),
            Some(format!("## {NOW}").as_str()),
            "for {before:?}"
        );
        assert_eq!(
            trailing_empty_section_line(&after),
            Some(line),
            "for {before:?}"
        );
    }
}

#[test]
fn poke_log_returns_the_line_of_a_reopened_section() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("R001-x.md");
    let content = "# T\n\n---\n\n## 2026-09-19 08:00\n\n";
    fs::write(&path, content).unwrap();

    assert_eq!(poke_log(&path, parse_timestamp(NOW).unwrap()).unwrap(), 5);
    assert_eq!(fs::read_to_string(&path).unwrap(), content);
}
