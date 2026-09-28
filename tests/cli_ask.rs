//! `sond index` and `sond ask <question>` — semantic search over the logs.
//!
//! Contract established here:
//!
//! - `index` builds `./sond/.index` from the logs; the first time, it asks
//!   before downloading the embedding model into the user's cache, and
//!   refuses when there is no terminal to ask in
//! - creating the index adds `sond/.index` to `./.gitignore`, if there is one
//! - `ask` refreshes the index itself, then prints the closest sections, best
//!   first: `<id>  <title>` then `  L<line>  ## <heading>  <score>`, hits
//!   separated by a blank line; `-n` sets how many (default 5)
//! - `ask` without an index is an error pointing at `sond index`
//! - `search` is untouched: it stays literal
//! - semantic search is the `ask` feature; a build without it keeps both
//!   commands but says how to get them
//!
//! Run with `cargo test --features ask --test cli_ask`. The tests marked
//! `#[ignore]` also need the model already installed in your cache (run
//! `sond index` once in a terminal); add `-- --ignored` to run them.

#![cfg(unix)]

mod common;

#[cfg(feature = "ask")]
use std::fs;
use std::path::PathBuf;

use common::Project;
use predicates::prelude::*;

const FIXTURES: [&str; 3] = [
    "R001-2026-09-18-boundary-condition.md",
    "R002-2026-09-19-fourier-features.md",
    "R003-2026-09-20-adaptive-timestep.md",
];

fn project() -> Project {
    Project::with_fixture_logs(&FIXTURES)
}

fn index_file(p: &Project) -> PathBuf {
    p.logs_dir().join(".index")
}

// ---------------------------------------------------------------------------
// CLI contract
// ---------------------------------------------------------------------------

#[test]
fn help_lists_index_and_ask() {
    Project::empty()
        .sond()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("index").and(predicate::str::contains("ask")));
}

#[test]
fn ask_has_help() {
    Project::empty()
        .sond()
        .args(["ask", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("<QUESTION>"));
}

#[test]
fn ask_requires_a_question() {
    project().sond().arg("ask").assert().failure();
}

#[cfg(feature = "ask")]
#[test]
fn blank_question_is_rejected() {
    project()
        .sond()
        .args(["ask", "  "])
        .assert()
        .failure()
        .stderr(predicate::str::contains("question"));
}

#[test]
fn zero_hits_is_rejected() {
    project()
        .sond()
        .args(["ask", "-n", "0", "why"])
        .assert()
        .failure();
}

// ---------------------------------------------------------------------------
// build without the `ask` feature
// ---------------------------------------------------------------------------

#[cfg(not(feature = "ask"))]
#[test]
fn without_the_feature_both_commands_say_how_to_get_it() {
    let p = project();
    for args in [&["index"][..], &["ask", "why is it unstable"][..]] {
        p.sond()
            .args(args)
            .assert()
            .failure()
            .stdout("")
            .stderr(predicate::str::contains(
                "cargo install sond --features ask",
            ));
    }
    assert!(!index_file(&p).exists());
}

// ---------------------------------------------------------------------------
// ask without a usable index
// ---------------------------------------------------------------------------

#[cfg(feature = "ask")]
#[test]
fn ask_without_an_index_points_at_sond_index() {
    let p = project();
    p.sond()
        .args(["ask", "why is it unstable"])
        .assert()
        .failure()
        .stdout("")
        .stderr(predicate::str::contains("sond index"));
    assert_eq!(p.log_names(), FIXTURES);
    assert!(!p.home.join(".cache").exists());
}

#[cfg(feature = "ask")]
#[test]
fn ask_without_a_logs_directory_points_at_sond_index() {
    let p = Project::empty();
    p.sond()
        .args(["ask", "why"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("sond index"));
    assert!(!p.logs_dir().exists());
}

#[cfg(feature = "ask")]
#[test]
fn a_damaged_index_is_an_error_not_a_panic() {
    let p = project();
    fs::write(index_file(&p), "not an index").unwrap();
    p.sond()
        .args(["ask", "why"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(".index").and(predicate::str::contains("panicked").not()));
}

// ---------------------------------------------------------------------------
// index without the model
// ---------------------------------------------------------------------------

#[cfg(feature = "ask")]
#[test]
fn index_without_logs_is_an_error_and_creates_nothing() {
    let p = Project::empty();
    p.sond()
        .arg("index")
        .assert()
        .failure()
        .stderr(predicate::str::contains("no logs"));
    assert!(!p.logs_dir().exists());
    assert!(!p.home.join(".cache").exists());
}

#[cfg(feature = "ask")]
#[test]
fn index_does_not_download_without_a_terminal() {
    let p = project();
    fs::write(p.dir.join(".gitignore"), "/target\n").unwrap();
    p.sond()
        .arg("index")
        .assert()
        .failure()
        .stderr(predicate::str::contains("terminal"));
    assert!(!index_file(&p).exists());
    assert_eq!(
        fs::read_to_string(p.dir.join(".gitignore")).unwrap(),
        "/target\n"
    );
    assert!(!p.home.join(".cache/sond").exists());
}

#[cfg(feature = "ask")]
#[test]
fn index_leaves_the_logs_untouched() {
    let p = project();
    p.sond().arg("index").assert().failure();
    assert_eq!(p.log_names(), FIXTURES);
}

// ---------------------------------------------------------------------------
// with the model installed (`--ignored`)
// ---------------------------------------------------------------------------

/// `sond` in the test project, but seeing the developer's real cache, where
/// the model is already installed.
#[cfg(feature = "ask")]
fn sond_with_model(p: &Project) -> assert_cmd::Command {
    let cache = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .filter(|c| c.is_absolute())
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap()).join(".cache"));
    let mut cmd = p.sond();
    cmd.env("XDG_CACHE_HOME", cache);
    cmd
}

#[cfg(feature = "ask")]
fn ask(p: &Project, question: &str) -> String {
    let out = sond_with_model(p)
        .args(["ask", question])
        .assert()
        .success();
    String::from_utf8(out.get_output().stdout.clone()).unwrap()
}

#[cfg(feature = "ask")]
#[test]
#[ignore = "needs the embedding model installed"]
fn index_adds_itself_to_an_existing_gitignore_once() {
    let p = project();
    fs::write(p.dir.join(".gitignore"), "/target\n").unwrap();
    sond_with_model(&p).arg("index").assert().success();
    sond_with_model(&p).arg("index").assert().success();
    assert!(index_file(&p).is_file());
    assert_eq!(
        fs::read_to_string(p.dir.join(".gitignore")).unwrap(),
        "/target\nsond/.index\n"
    );
}

#[cfg(feature = "ask")]
#[test]
#[ignore = "needs the embedding model installed"]
fn index_does_not_create_a_gitignore() {
    let p = project();
    sond_with_model(&p).arg("index").assert().success();
    assert!(!p.dir.join(".gitignore").exists());
}

#[cfg(feature = "ask")]
#[test]
#[ignore = "needs the embedding model installed"]
fn ask_finds_a_paraphrase() {
    let p = project();
    sond_with_model(&p).arg("index").assert().success();
    let out = ask(&p, "why does the solver blow up with larger time steps");
    assert!(
        out.starts_with("R003  Adaptive timestep instability\n  L"),
        "{out}"
    );
}

#[cfg(feature = "ask")]
#[test]
#[ignore = "needs the embedding model installed"]
fn ask_prints_five_hits_by_default_and_n_on_request() {
    let p = project();
    sond_with_model(&p).arg("index").assert().success();
    let hits = |out: &str| out.lines().filter(|l| l.starts_with("  L")).count();
    assert_eq!(hits(&ask(&p, "energy")), 5);
    let out = sond_with_model(&p)
        .args(["ask", "-n", "2", "energy"])
        .assert()
        .success();
    assert_eq!(hits(&String::from_utf8_lossy(&out.get_output().stdout)), 2);
}

#[cfg(feature = "ask")]
#[test]
#[ignore = "needs the embedding model installed"]
fn ask_prints_id_title_line_heading_and_score() {
    let p = project();
    sond_with_model(&p).arg("index").assert().success();
    let out = ask(&p, "ghost cells and flux correction order");
    let mut lines = out.lines();
    assert_eq!(lines.next(), Some("R001  Boundary condition leaks energy"));
    let hit = lines.next().unwrap();
    assert!(hit.starts_with("  L10  ## Investigation  0."), "{hit}");
}

#[cfg(feature = "ask")]
#[test]
#[ignore = "needs the embedding model installed"]
fn ask_sees_sections_added_since_the_last_index() {
    let p = project();
    sond_with_model(&p).arg("index").assert().success();
    p.sond()
        .env(
            "FAKE_EDITOR_APPEND",
            "Switching to a spectral element basis removed the drift.",
        )
        .args(["poke", "-e", "R002"])
        .assert()
        .success();
    let out = ask(&p, "which discretisation fixed the drift");
    assert!(
        out.starts_with(
            "R002  Fourier features for the surrogate model\n  L16  ## 2026-09-21 12:19"
        ),
        "{out}"
    );
}
