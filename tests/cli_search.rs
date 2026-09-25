//! `sond search <query>` — find past investigations.
//!
//! Contract established here:
//!
//! - the query is a literal, case-insensitive phrase; unquoted words are
//!   joined with single spaces, like `new` titles
//! - every line of every log in `./sond/` is searched, titles included;
//!   files that are not logs are not
//! - output is grouped by log, most recently active first (as `recent`):
//!   `<id>  <title>` then `<line number, width 4>: <line>`, groups separated
//!   by a blank line
//! - exit 0 when something matched; exit 1 with empty stdout when nothing
//!   did (like grep), with a note on stderr
//! - read-only: no file is written, no editor opened, the clock is not read

#![cfg(unix)]

mod common;

use std::fs;

use common::{Project, fixture};
use predicates::prelude::*;

const FIXTURES: [&str; 3] = [
    "R001-2026-09-18-boundary-condition.md",
    "R002-2026-09-19-fourier-features.md",
    "R003-2026-09-20-adaptive-timestep.md",
];

fn project() -> Project {
    Project::with_fixture_logs(&FIXTURES)
}

fn search(p: &Project, query: &[&str]) -> String {
    let out = p.sond().arg("search").args(query).assert().success();
    String::from_utf8(out.get_output().stdout.clone()).unwrap()
}

// ---------------------------------------------------------------------------
// CLI contract
// ---------------------------------------------------------------------------

#[test]
fn help_lists_search() {
    Project::empty()
        .sond()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("search"));
}

#[test]
fn search_has_help() {
    Project::empty()
        .sond()
        .args(["search", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("<QUERY>"));
}

#[test]
fn search_requires_a_query() {
    project().sond().arg("search").assert().failure();
}

#[test]
fn blank_query_is_rejected() {
    project()
        .sond()
        .args(["search", "  "])
        .assert()
        .failure()
        .stderr(predicate::str::contains("query"));
}

// ---------------------------------------------------------------------------
// matching
// ---------------------------------------------------------------------------

#[test]
fn finds_body_content() {
    assert_eq!(
        search(&project(), &["CFL"]),
        "\
R003  Adaptive timestep instability
  12: Initial experiments suggest the instability is related to the CFL condition.
"
    );
}

#[test]
fn is_case_insensitive() {
    assert_eq!(search(&project(), &["cfl"]), search(&project(), &["CFL"]));
}

#[test]
fn finds_titles() {
    assert_eq!(
        search(&project(), &["fourier features for"]),
        "R002  Fourier features for the surrogate model\n   1: # Fourier features for the surrogate model\n"
    );
}

#[test]
fn lists_every_matching_line_of_a_log() {
    assert_eq!(
        search(&project(), &["instability"]),
        "\
R003  Adaptive timestep instability
   1: # Adaptive timestep instability
  12: Initial experiments suggest the instability is related to the CFL condition.
"
    );
}

#[test]
fn searches_across_logs_most_recent_first() {
    assert_eq!(
        search(&project(), &["## Investigation"]),
        "\
R003  Adaptive timestep instability
  10: ## Investigation

R002  Fourier features for the surrogate model
  10: ## Investigation

R001  Boundary condition leaks energy
  10: ## Investigation
"
    );
}

#[test]
fn a_poke_moves_a_log_up_the_results() {
    let p = project();
    p.sond().args(["poke", "R001"]).assert().success();
    let out = search(&p, &["## Investigation"]);
    assert!(out.starts_with("R001  "), "{out}");
}

#[test]
fn unquoted_words_are_one_phrase() {
    let p = project();
    assert_eq!(
        search(&p, &["CFL", "condition"]),
        search(&p, &["CFL condition"])
    );
    p.sond()
        .args(["search", "condition", "CFL"])
        .assert()
        .code(1);
}

#[test]
fn queries_are_literal() {
    let p = Project::empty();
    p.add_log(
        "R001-2026-09-18-symbols.md",
        "# Symbols\n\nu_t + c*u_x = 0 (advection)? see a.b/c [1] \\nabla\n",
    );
    for q in [
        "+",
        "?",
        ".",
        "*",
        "/",
        "c*u_x",
        "(advection)?",
        "[1]",
        "\\nabla",
    ] {
        p.sond()
            .args(["search", q])
            .assert()
            .success()
            .stdout(predicate::str::contains("R001  Symbols"));
    }
    for q in ["u.*0", "a.c", "[0-9]"] {
        p.sond().args(["search", q]).assert().code(1);
    }
}

#[test]
fn accented_capitals_match() {
    let p = Project::empty();
    p.add_log("R001-2026-09-18-precision.md", "# Précision numérique\n");
    assert_eq!(
        search(&p, &["PRÉCISION"]),
        "R001  Précision numérique\n   1: # Précision numérique\n"
    );
}

#[test]
fn only_logs_are_searched() {
    let p = project();
    p.add_log("README.md", "CFL everywhere\n");
    fs::write(p.dir.join("notes.md"), "CFL here too\n").unwrap();
    assert_eq!(search(&p, &["CFL"]).lines().count(), 2);
}

#[test]
fn text_added_after_a_poke_is_found() {
    let p = project();
    p.sond()
        .env("FAKE_EDITOR_APPEND", "Ghost cells are updated too late.")
        .args(["poke", "-e", "R001"])
        .assert()
        .success();
    assert_eq!(
        search(&p, &["ghost cells"]),
        "R001  Boundary condition leaks energy\n  18: Ghost cells are updated too late.\n"
    );
}

#[test]
fn non_utf8_logs_are_still_searched() {
    let p = Project::empty();
    fs::create_dir(p.logs_dir()).unwrap();
    fs::write(
        p.logs_dir().join("R001-2026-09-18-latin1.md"),
        b"# Old \xe9ncoding\n\nCFL in latin-1\n",
    )
    .unwrap();
    assert_eq!(
        search(&p, &["cfl"]),
        "R001  Old \u{fffd}ncoding\n   3: CFL in latin-1\n"
    );
}

// ---------------------------------------------------------------------------
// no results
// ---------------------------------------------------------------------------

#[test]
fn no_match_exits_1_with_empty_stdout() {
    project()
        .sond()
        .args(["search", "navier-stokes"])
        .assert()
        .code(1)
        .stdout("")
        .stderr(predicate::str::contains("navier-stokes"));
}

#[test]
fn no_logs_directory_is_no_match_and_is_not_created() {
    let p = Project::empty();
    p.sond().args(["search", "CFL"]).assert().code(1).stdout("");
    assert!(!p.logs_dir().exists());
}

// ---------------------------------------------------------------------------
// read-only
// ---------------------------------------------------------------------------

#[test]
fn changes_nothing_opens_nothing_and_ignores_the_clock() {
    let p = project();
    p.sond()
        .env("SOND_NOW", "not a time")
        .args(["search", "CFL"])
        .assert()
        .success();
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
fn logs_path_that_is_a_file_is_an_error() {
    let p = Project::empty();
    fs::write(p.logs_dir(), "CFL").unwrap();
    p.sond()
        .args(["search", "CFL"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("sond").and(predicate::str::contains("panicked").not()));
}
