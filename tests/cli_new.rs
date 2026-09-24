//! `sond new <title>` — start a new investigation.
//!
//! Contract established here:
//!
//! - logs live in `./logs/`, created on demand
//! - filename is `R<id>-<YYYY-MM-DD>-<slug>.md`
//! - the ID is one more than the highest existing ID, starting at R001
//! - content is the configured template (or the default) with `title`, `id`,
//!   `created` and `date` substituted
//! - stdout is the relative path of the new log, one line
//! - no editor is opened unless asked for: with no `$VISUAL` or `$EDITOR`
//!   set, `sond new` still just creates the log and succeeds
//! - with `-e` / `--edit`, the log is then opened with `$VISUAL`, `$EDITOR`,
//!   or `vi`
//! - an editor failure is an error, but the log is kept

#![cfg(unix)]

mod common;

use std::fs;

use common::{NOW, Project, fixture, sh_editor};
use predicates::prelude::*;

const TITLE: &str = "Adaptive timestep instability";
const FILENAME: &str = "R001-2026-09-21-adaptive-timestep-instability.md";

// ---------------------------------------------------------------------------
// CLI contract
// ---------------------------------------------------------------------------

#[test]
fn no_subcommand_is_an_error() {
    Project::empty().sond().assert().failure();
}

#[test]
fn unknown_subcommand_is_an_error() {
    Project::empty()
        .sond()
        .arg("unknown-command")
        .assert()
        .failure();
}

#[test]
fn help_lists_new() {
    Project::empty()
        .sond()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("new"));
}

#[test]
fn new_has_help() {
    Project::empty()
        .sond()
        .args(["new", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("<TITLE>"));
}

#[test]
fn new_requires_a_title() {
    let p = Project::empty();
    p.sond().arg("new").assert().failure();
    assert!(p.log_names().is_empty());
}

#[test]
fn blank_title_is_rejected_without_creating_a_log() {
    let p = Project::empty();
    p.sond()
        .args(["new", "   "])
        .assert()
        .failure()
        .stderr(predicate::str::contains("title"));
    assert!(p.log_names().is_empty());
    assert!(p.editor_invocations().is_empty());
}

// ---------------------------------------------------------------------------
// the created file
// ---------------------------------------------------------------------------

#[test]
fn creates_the_logs_directory() {
    let p = Project::empty();
    assert!(!p.logs_dir().exists());
    p.sond().args(["new", TITLE]).assert().success();
    assert!(p.logs_dir().is_dir());
}

#[test]
fn creates_exactly_one_markdown_log_with_a_deterministic_name() {
    let p = Project::empty();
    p.sond().args(["new", TITLE]).assert().success();
    assert_eq!(p.log_names(), [FILENAME]);
}

#[test]
fn prints_the_path_of_the_new_log() {
    Project::empty()
        .sond()
        .args(["new", TITLE])
        .assert()
        .success()
        .stdout(format!("logs/{FILENAME}\n"));
}

#[test]
fn default_template_produces_this_document() {
    let p = Project::empty();
    p.sond().args(["new", TITLE]).assert().success();
    assert_eq!(
        p.read_log(FILENAME),
        format!(
            "# {TITLE}

Created: {NOW}
ID: R001

## Context

## Investigation

## Next steps
"
        )
    );
}

#[test]
fn unquoted_words_form_one_title() {
    let p = Project::empty();
    p.sond()
        .args(["new", "Adaptive", "timestep", "instability"])
        .assert()
        .success();
    assert_eq!(p.log_names(), [FILENAME]);
    assert!(p.read_log(FILENAME).starts_with(&format!("# {TITLE}\n")));
}

#[test]
fn title_is_kept_verbatim_in_the_document() {
    // The slug is lossy; the heading must not be.
    let p = Project::empty();
    let title = "Précision numérique: why does Δt > 0.01 diverge?";
    p.sond().args(["new", title]).assert().success();
    let name = &p.log_names()[0];
    assert_eq!(
        name,
        "R001-2026-09-21-precision-numerique-why-does-t-0-01-diverge.md"
    );
    assert!(p.read_log(name).starts_with(&format!("# {title}\n")));
}

#[test]
fn created_date_comes_from_the_clock() {
    let p = Project::empty();
    p.sond()
        .env("SOND_NOW", "2027-01-02 03:04")
        .args(["new", "x"])
        .assert()
        .success();
    let name = "R001-2027-01-02-x.md";
    assert_eq!(p.log_names(), [name]);
    assert!(p.read_log(name).contains("Created: 2027-01-02 03:04\n"));
}

#[test]
fn invalid_clock_override_is_an_error() {
    let p = Project::empty();
    p.sond()
        .env("SOND_NOW", "yesterday")
        .args(["new", TITLE])
        .assert()
        .failure()
        .stderr(predicate::str::contains("SOND_NOW").or(predicate::str::contains("timestamp")));
    assert!(p.log_names().is_empty());
}

// ---------------------------------------------------------------------------
// slugs
// ---------------------------------------------------------------------------

#[test]
fn slugs_are_filesystem_safe() {
    let cases = [
        ("Why does it   DIVERGE?!", "why-does-it-diverge"),
        ("Élément fini / cœur", "element-fini-coeur"),
        ("../../etc/passwd", "etc-passwd"),
        ("日本語", "untitled"),
        (
            "This is an extremely long research log title that certainly exceeds the sixty character limit",
            "this-is-an-extremely-long-research-log-title-that-certainly",
        ),
    ];
    for (title, slug) in cases {
        let p = Project::empty();
        p.sond().args(["new", title]).assert().success();
        assert_eq!(
            p.log_names(),
            [format!("R001-2026-09-21-{slug}.md")],
            "for title {title:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// IDs
// ---------------------------------------------------------------------------

#[test]
fn consecutive_logs_get_distinct_increasing_ids() {
    let p = Project::empty();
    p.sond().args(["new", TITLE]).assert().success();
    p.sond().args(["new", TITLE]).assert().success();
    assert_eq!(
        p.log_names(),
        [
            "R001-2026-09-21-adaptive-timestep-instability.md",
            "R002-2026-09-21-adaptive-timestep-instability.md",
        ]
    );
    assert!(p.read_log(&p.log_names()[1]).contains("ID: R002\n"));
}

#[test]
fn next_id_follows_the_highest_existing_one() {
    let cases: [(&[&str], &str); 3] = [
        (&["R001-2026-09-01-a.md"], "R002"),
        (&["R001-2026-09-01-a.md", "R003-2026-09-03-c.md"], "R004"),
        (
            &[
                "R001-2026-09-01-a.md",
                "R002-2026-09-02-b.md",
                "R010-2026-09-10-j.md",
            ],
            "R011",
        ),
    ];
    for (existing, expected) in cases {
        let p = Project::empty();
        for name in existing {
            p.add_log(name, "# old\n");
        }
        p.sond().args(["new", "next"]).assert().success();
        let created = format!("{expected}-2026-09-21-next.md");
        assert!(
            p.log_names().contains(&created),
            "after {existing:?} expected {created}, got {:?}",
            p.log_names()
        );
    }
}

#[test]
fn ids_count_renamed_and_widened_logs() {
    let p = Project::empty();
    p.add_log("R007-my-renamed-notes.md", "");
    p.add_log("R1000-2026-01-01-wide.md", "");
    p.sond().args(["new", "next"]).assert().success();
    assert!(
        p.log_names()
            .contains(&"R1001-2026-09-21-next.md".to_string())
    );
}

#[test]
fn files_that_are_not_logs_are_ignored() {
    let p = Project::empty();
    p.add_log("README.md", "");
    p.add_log("R999.txt", "");
    p.add_log("notes-R050.md", "");
    fs::create_dir(p.logs_dir().join("figures")).unwrap();
    p.sond().args(["new", "first"]).assert().success();
    assert!(
        p.log_names()
            .contains(&"R001-2026-09-21-first.md".to_string())
    );
}

#[test]
fn existing_logs_are_left_byte_for_byte_untouched() {
    let names = [
        "R001-2026-09-18-boundary-condition.md",
        "R002-2026-09-19-fourier-features.md",
        "R003-2026-09-20-adaptive-timestep.md",
    ];
    let p = Project::with_fixture_logs(&names);
    p.sond().args(["new", TITLE]).assert().success();

    for name in names {
        let original = fs::read(fixture(&format!("logs/{name}"))).unwrap();
        assert_eq!(
            fs::read(p.logs_dir().join(name)).unwrap(),
            original,
            "{name}"
        );
    }
    assert_eq!(p.log_names().len(), 4);
    assert!(
        p.log_names()
            .contains(&"R004-2026-09-21-adaptive-timestep-instability.md".to_string())
    );
}

// ---------------------------------------------------------------------------
// templates
// ---------------------------------------------------------------------------

#[test]
fn configured_template_is_applied() {
    let p = Project::empty();
    p.set_template(&fs::read_to_string(fixture("templates/custom.md")).unwrap());
    p.sond().args(["new", TITLE]).assert().success();
    assert_eq!(
        p.read_log(FILENAME),
        format!(
            "# Investigation: {TITLE}

Identifier: R001
Started: {NOW}
Day: 2026-09-21
Reviewer: {{{{ reviewer }}}}

## Hypothesis

## Evidence

## Conclusion
"
        )
    );
}

#[test]
fn minimal_template_is_applied() {
    let p = Project::empty();
    p.set_template(&fs::read_to_string(fixture("templates/minimal.md")).unwrap());
    p.sond().args(["new", TITLE]).assert().success();
    assert_eq!(
        p.read_log(FILENAME),
        format!("# {TITLE}\n\nCreated: {NOW}\nID: R001\n\n## Notes\n")
    );
}

#[test]
fn template_falls_back_to_home_dot_config() {
    let p = Project::empty();
    let dir = p.home.join(".config/sond");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("template.md"), "from home: {{ id }}\n").unwrap();

    p.sond()
        .env_remove("XDG_CONFIG_HOME")
        .args(["new", TITLE])
        .assert()
        .success();
    assert_eq!(p.read_log(FILENAME), "from home: R001\n");
}

#[test]
fn unreadable_template_is_an_error_and_creates_nothing() {
    let p = Project::empty();
    // A directory where the template file should be.
    fs::create_dir_all(p.config.join("sond/template.md")).unwrap();
    p.sond()
        .args(["new", TITLE])
        .assert()
        .failure()
        .stderr(predicate::str::contains("template"));
    assert!(p.log_names().is_empty());
    assert!(p.editor_invocations().is_empty());
}

// ---------------------------------------------------------------------------
// editor
// ---------------------------------------------------------------------------

#[test]
fn does_not_open_an_editor_by_default() {
    let p = Project::empty();
    p.sond()
        .args(["new", TITLE])
        .assert()
        .success()
        .stdout(format!("logs/{FILENAME}\n"));
    assert_eq!(p.log_names(), [FILENAME]);
    assert!(p.editor_invocations().is_empty());
}

#[test]
fn without_any_editor_configured_the_log_is_just_created() {
    // The user's real setup: neither `$VISUAL` nor `$EDITOR` is set. Sond
    // must not fall back to an interactive program such as `vi`.
    let p = Project::empty();
    p.sond_without_editor()
        .args(["new", TITLE])
        .assert()
        .success()
        .stdout(format!("logs/{FILENAME}\n"));
    assert_eq!(p.log_names(), [FILENAME]);

    assert!(p.editor_invocations().is_empty());
}

#[test]
fn edit_flag_opens_the_new_log_in_the_editor_once() {
    for flag in ["-e", "--edit"] {
        let p = Project::empty();
        p.sond().args(["new", flag, TITLE]).assert().success();
        assert_eq!(
            p.editor_invocations(),
            [[format!("logs/{FILENAME}")]],
            "for {flag}"
        );
    }
}

#[test]
fn edit_flag_may_follow_the_title() {
    let p = Project::empty();
    p.sond()
        .args(["new", "Adaptive", "timestep", "instability", "-e"])
        .assert()
        .success();
    assert_eq!(p.log_names(), [FILENAME]);
    assert_eq!(p.editor_invocations(), [[format!("logs/{FILENAME}")]]);
}

#[test]
fn a_title_can_still_contain_e_after_a_double_dash() {
    let p = Project::empty();
    p.sond()
        .args(["new", "--", "-e", "mode"])
        .assert()
        .success();
    assert_eq!(p.log_names(), ["R001-2026-09-21-e-mode.md"]);
    assert!(p.editor_invocations().is_empty());
}

#[test]
fn what_the_user_types_in_the_editor_lands_in_the_log() {
    let p = Project::empty();
    p.sond()
        .env(
            "FAKE_EDITOR_APPEND",
            "First observation: dt > 0.01 blows up.",
        )
        .args(["new", "-e", TITLE])
        .assert()
        .success();
    assert!(
        p.read_log(FILENAME)
            .ends_with("## Next steps\nFirst observation: dt > 0.01 blows up.\n")
    );
}

#[test]
fn editor_flags_are_passed_through() {
    // Stands in for `EDITOR="code --wait"`.
    let p = Project::empty();
    p.sond()
        .env(
            "EDITOR",
            format!("{} --wait", sh_editor("editor/fake-editor.sh")),
        )
        .args(["new", "-e", TITLE])
        .assert()
        .success();
    assert_eq!(
        p.editor_invocations(),
        [["--wait".to_string(), format!("logs/{FILENAME}")]]
    );
}

#[test]
fn visual_takes_precedence_over_editor() {
    let p = Project::empty();
    p.sond()
        .env("VISUAL", sh_editor("editor/fake-editor.sh"))
        .env("EDITOR", sh_editor("editor/failing-editor.sh"))
        .args(["new", "-e", TITLE])
        .assert()
        .success();
    assert_eq!(p.editor_invocations().len(), 1);
}

#[test]
fn blank_visual_falls_back_to_editor() {
    let p = Project::empty();
    p.sond()
        .env("VISUAL", "  ")
        .args(["new", "-e", TITLE])
        .assert()
        .success();
    assert_eq!(p.editor_invocations().len(), 1);
}

#[test]
fn failing_editor_is_an_error_but_the_log_is_kept() {
    let p = Project::empty();
    p.sond()
        .env("EDITOR", sh_editor("editor/failing-editor.sh"))
        .args(["new", "-e", TITLE])
        .assert()
        .failure()
        .stdout(format!("logs/{FILENAME}\n"))
        .stderr(predicate::str::contains("editor"));
    assert_eq!(p.log_names(), [FILENAME]);
}

#[test]
fn missing_editor_is_an_error_but_the_log_is_kept() {
    let p = Project::empty();
    p.sond()
        .env("EDITOR", "/nonexistent/editor")
        .args(["new", "-e", TITLE])
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("/nonexistent/editor")
                .and(predicate::str::contains("panicked").not()),
        );
    assert_eq!(p.log_names(), [FILENAME]);
}

// ---------------------------------------------------------------------------
// filesystem failures
// ---------------------------------------------------------------------------

#[test]
fn logs_path_that_is_a_file_is_an_error_not_a_panic() {
    let p = Project::empty();
    fs::write(p.logs_dir(), "not a directory").unwrap();
    p.sond()
        .args(["new", TITLE])
        .assert()
        .failure()
        .stderr(predicate::str::contains("logs").and(predicate::str::contains("panicked").not()));
    assert_eq!(fs::read_to_string(p.logs_dir()).unwrap(), "not a directory");
    assert!(p.editor_invocations().is_empty());
}

#[test]
fn works_outside_a_git_repository() {
    let p = Project::empty();
    assert!(!p.dir.join(".git").exists());
    p.sond()
        .env("GIT_DIR", "/nonexistent")
        .args(["new", TITLE])
        .assert()
        .success();
    assert_eq!(p.log_names(), [FILENAME]);
}
