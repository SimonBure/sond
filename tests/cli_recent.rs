//! `sond recent` — what have I been working on lately?
//!
//! Contract established here:
//!
//! - one line per log: `<id>  <YYYY-MM-DD HH:MM>  <title>`, IDs padded to
//!   the widest one so the columns line up
//! - activity time is the latest of the `Created:` line, the newest
//!   `## YYYY-MM-DD HH:MM` section, and the filename date (at midnight); never
//!   the filesystem mtime. A log with none of these shows `-` and sorts last
//! - newest first; ties broken by the higher ID first
//! - at most 10 lines, or `-n/--limit N` (N >= 1)
//! - title is the first `# ` heading, falling back to the filename slug
//! - read-only: no file is written, no editor opened, the clock is not read
//! - no logs is not an error: empty stdout, exit 0, a hint on stderr

#![cfg(unix)]

mod common;

use std::fs::{self, File};
use std::time::{Duration, SystemTime};

use common::{Project, fixture};
use predicates::prelude::*;

const FIXTURES: [&str; 3] = [
    "R001-2026-09-18-boundary-condition.md",
    "R002-2026-09-19-fourier-features.md",
    "R003-2026-09-20-adaptive-timestep.md",
];

const FIXTURES_RECENT: &str = "\
R003  2026-09-20 10:15  Adaptive timestep instability
R002  2026-09-19 14:05  Fourier features for the surrogate model
R001  2026-09-18 09:30  Boundary condition leaks energy
";

fn project() -> Project {
    Project::with_fixture_logs(&FIXTURES)
}

fn recent(p: &Project) -> String {
    let out = p.sond().arg("recent").assert().success();
    String::from_utf8(out.get_output().stdout.clone()).unwrap()
}

// ---------------------------------------------------------------------------
// CLI contract
// ---------------------------------------------------------------------------

#[test]
fn help_lists_recent() {
    Project::empty()
        .sond()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("recent"));
}

#[test]
fn recent_help_documents_the_limit() {
    Project::empty()
        .sond()
        .args(["recent", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--limit"));
}

#[test]
fn limit_must_be_a_positive_number() {
    let p = project();
    for bad in ["0", "-1", "ten"] {
        p.sond().args(["recent", "--limit", bad]).assert().failure();
    }
}

#[test]
fn recent_takes_no_positional_arguments() {
    project().sond().args(["recent", "R001"]).assert().failure();
}

// ---------------------------------------------------------------------------
// empty state
// ---------------------------------------------------------------------------

#[test]
fn no_logs_directory_is_a_clean_empty_state() {
    let p = Project::empty();
    p.sond()
        .arg("recent")
        .assert()
        .success()
        .stdout("")
        .stderr(predicate::str::contains("sond new"));
    assert!(!p.logs_dir().exists(), "recent must not create logs/");
}

#[test]
fn empty_logs_directory_is_a_clean_empty_state() {
    let p = Project::empty();
    fs::create_dir(p.logs_dir()).unwrap();
    p.add_log("README.md", "not a log\n");
    p.sond().arg("recent").assert().success().stdout("");
}

// ---------------------------------------------------------------------------
// ordering and format
// ---------------------------------------------------------------------------

#[test]
fn lists_newest_first() {
    assert_eq!(recent(&project()), FIXTURES_RECENT);
}

#[test]
fn output_is_deterministic() {
    let p = project();
    assert_eq!(recent(&p), recent(&p));
}

#[test]
fn a_poke_brings_an_old_investigation_to_the_top() {
    let p = project();
    p.sond().args(["poke", "R001"]).assert().success();
    assert_eq!(
        recent(&p),
        "\
R001  2026-09-21 12:19  Boundary condition leaks energy
R003  2026-09-20 10:15  Adaptive timestep instability
R002  2026-09-19 14:05  Fourier features for the surrogate model
"
    );
}

#[test]
fn filesystem_mtime_is_ignored() {
    // Touching the oldest log (an editor save, a `git checkout`) must not
    // reorder anything: only what Sond wrote into the file counts.
    let p = project();
    let file = File::options()
        .append(true)
        .open(p.logs_dir().join(FIXTURES[0]))
        .unwrap();
    file.set_modified(SystemTime::now() + Duration::from_secs(86_400))
        .unwrap();
    let file = File::options()
        .append(true)
        .open(p.logs_dir().join(FIXTURES[2]))
        .unwrap();
    file.set_modified(SystemTime::UNIX_EPOCH).unwrap();

    assert_eq!(recent(&p), FIXTURES_RECENT);
}

#[test]
fn ties_put_the_higher_id_first() {
    let p = Project::empty();
    for name in ["R001-2026-09-20-a.md", "R002-2026-09-20-b.md"] {
        p.add_log(name, "# Same minute\n\nCreated: 2026-09-20 10:00\n");
    }
    assert_eq!(
        recent(&p),
        "\
R002  2026-09-20 10:00  Same minute
R001  2026-09-20 10:00  Same minute
"
    );
}

#[test]
fn ids_are_padded_so_columns_line_up() {
    let p = Project::empty();
    p.add_log("R999-2026-09-01-a.md", "# Old\n");
    p.add_log("R1000-2026-09-02-b.md", "# New\n");
    assert_eq!(
        recent(&p),
        "\
R1000  2026-09-02 00:00  New
R999   2026-09-01 00:00  Old
"
    );
}

// ---------------------------------------------------------------------------
// tolerance for hand-edited and custom logs
// ---------------------------------------------------------------------------

#[test]
fn custom_templates_without_created_fall_back_to_the_filename_date() {
    let p = Project::empty();
    p.set_template(&fs::read_to_string(fixture("templates/custom.md")).unwrap());
    p.sond().args(["new", "CFL"]).assert().success();
    assert_eq!(recent(&p), "R001  2026-09-21 00:00  Investigation: CFL\n");
}

#[test]
fn title_falls_back_to_the_slug() {
    let p = Project::empty();
    p.add_log("R001-2026-09-18-no-heading-here.md", "just text\n");
    assert_eq!(recent(&p), "R001  2026-09-18 00:00  no-heading-here\n");
}

#[test]
fn logs_without_any_date_are_listed_last() {
    let p = project();
    p.add_log("R004-renamed-notes.md", "# Undated\n");
    assert_eq!(
        recent(&p),
        format!("{FIXTURES_RECENT}R004  -                 Undated\n")
    );
}

#[test]
fn non_utf8_logs_are_still_listed() {
    let p = Project::empty();
    fs::create_dir(p.logs_dir()).unwrap();
    fs::write(
        p.logs_dir().join("R001-2026-09-18-latin1.md"),
        b"# \xe9t\xe9\n\nCreated: 2026-09-18 09:30\n",
    )
    .unwrap();
    assert_eq!(recent(&p), "R001  2026-09-18 09:30  \u{fffd}t\u{fffd}\n");
}

// ---------------------------------------------------------------------------
// limit
// ---------------------------------------------------------------------------

fn twelve_logs() -> Project {
    let p = Project::empty();
    for day in 1..=12 {
        p.add_log(
            &format!("R{day:03}-2026-09-{day:02}-log.md"),
            &format!("# Log {day}\n"),
        );
    }
    p
}

#[test]
fn shows_ten_by_default() {
    let out = recent(&twelve_logs());
    let ids: Vec<&str> = out.lines().map(|l| &l[..4]).collect();
    assert_eq!(
        ids,
        [
            "R012", "R011", "R010", "R009", "R008", "R007", "R006", "R005", "R004", "R003"
        ]
    );
}

#[test]
fn limit_shows_the_n_newest() {
    for flag in ["-n", "--limit"] {
        twelve_logs()
            .sond()
            .args(["recent", flag, "2"])
            .assert()
            .success()
            .stdout("R012  2026-09-12 00:00  Log 12\nR011  2026-09-11 00:00  Log 11\n");
    }
}

#[test]
fn limit_larger_than_the_log_count_shows_everything() {
    project()
        .sond()
        .args(["recent", "-n", "100"])
        .assert()
        .success()
        .stdout(FIXTURES_RECENT);
}

// ---------------------------------------------------------------------------
// read-only
// ---------------------------------------------------------------------------

#[test]
fn changes_nothing_and_opens_nothing() {
    let p = project();
    recent(&p);
    assert_eq!(p.log_names(), FIXTURES);
    for name in FIXTURES {
        assert_eq!(
            fs::read(p.logs_dir().join(name)).unwrap(),
            fs::read(fixture(&format!("logs/{name}"))).unwrap()
        );
    }
    assert!(p.editor_invocations().is_empty());
}

#[test]
fn does_not_depend_on_the_clock() {
    let p = project();
    for now in ["2020-01-01 00:00", "not even a time"] {
        p.sond()
            .env("SOND_NOW", now)
            .arg("recent")
            .assert()
            .success()
            .stdout(FIXTURES_RECENT);
    }
}

#[test]
fn logs_path_that_is_a_file_is_an_error() {
    let p = Project::empty();
    fs::write(p.logs_dir(), "oops").unwrap();
    p.sond()
        .arg("recent")
        .assert()
        .failure()
        .stderr(predicate::str::contains("logs").and(predicate::str::contains("panicked").not()));
}

// ---------------------------------------------------------------------------
// workflow
// ---------------------------------------------------------------------------

#[test]
fn new_new_poke_recent() {
    let p = Project::empty();
    p.sond()
        .env("SOND_NOW", "2026-09-18 09:00")
        .args(["new", "Boundary leak"])
        .assert()
        .success();
    p.sond()
        .env("SOND_NOW", "2026-09-19 09:00")
        .args(["new", "Fourier features"])
        .assert()
        .success();
    p.sond()
        .env("SOND_NOW", "2026-09-20 17:45")
        .args(["poke", "R001"])
        .assert()
        .success();

    assert_eq!(
        recent(&p),
        "\
R001  2026-09-20 17:45  Boundary leak
R002  2026-09-19 09:00  Fourier features
"
    );
}
