//! `sond template set <file>` and `sond template edit`.
//!
//! Contract established here:
//!
//! - the template lives at `$XDG_CONFIG_HOME/sond/template.md`, falling back
//!   to `~/.config/sond/template.md`; directories are created on demand
//! - `set` stores a *copy* of the file; later edits to the source do not leak
//! - `set` replaces the template atomically: on any failure the previous
//!   template is untouched and no temporary file is left behind
//! - `set` rejects sources that are missing, directories, unreadable, or not
//!   UTF-8; it does not validate placeholders
//! - `edit` opens the template, first writing the default if there is none;
//!   it never overwrites an existing template
//! - both print the template's path, one line, and never touch `sond/`

#![cfg(unix)]

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;

use common::{Project, fixture, sh_editor};
use predicates::prelude::*;
use sond::template::DEFAULT_TEMPLATE;

const PREVIOUS: &str = "# previous template {{ title }}\n";

fn custom() -> String {
    fs::read_to_string(fixture("templates/custom.md")).unwrap()
}

/// Asserts the template is exactly `content` and nothing else sits beside it.
fn assert_template(p: &Project, content: &str) {
    assert_eq!(fs::read_to_string(p.template_file()).unwrap(), content);
    assert_eq!(
        p.config_names(),
        ["template.md"],
        "stray files next to the template"
    );
}

/// True when this process ignores file permissions (running as root).
fn permissions_ignored(p: &Project) -> bool {
    let canary = p.home.join("perm-canary");
    fs::write(&canary, "").unwrap();
    fs::set_permissions(&canary, fs::Permissions::from_mode(0o000)).unwrap();
    let ignored = fs::read(&canary).is_ok();
    fs::remove_file(&canary).unwrap();
    ignored
}

// ---------------------------------------------------------------------------
// CLI contract
// ---------------------------------------------------------------------------

#[test]
fn help_lists_template() {
    Project::empty()
        .sond()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("template"));
}

#[test]
fn template_help_lists_set_and_edit() {
    Project::empty()
        .sond()
        .args(["template", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("set").and(predicate::str::contains("edit")));
}

#[test]
fn template_requires_a_subcommand() {
    Project::empty().sond().arg("template").assert().failure();
}

#[test]
fn unknown_template_subcommand_is_an_error() {
    Project::empty()
        .sond()
        .args(["template", "show"])
        .assert()
        .failure();
}

#[test]
fn set_requires_a_file() {
    let p = Project::empty();
    p.sond().args(["template", "set"]).assert().failure();
    assert!(!p.template_file().exists());
}

// ---------------------------------------------------------------------------
// template set
// ---------------------------------------------------------------------------

#[test]
fn set_installs_a_copy_and_prints_where() {
    let p = Project::empty();
    p.sond()
        .args(["template", "set"])
        .arg(fixture("templates/custom.md"))
        .assert()
        .success()
        .stdout(format!("{}\n", p.template_file().display()));
    assert_template(&p, &custom());
}

#[test]
fn set_accepts_a_path_relative_to_the_working_directory() {
    let p = Project::empty();
    fs::write(p.dir.join("mine.md"), "# mine: {{ title }}\n").unwrap();
    p.sond()
        .args(["template", "set", "mine.md"])
        .assert()
        .success();
    assert_template(&p, "# mine: {{ title }}\n");
}

#[test]
fn set_replaces_the_previous_template() {
    let p = Project::empty();
    p.set_template(PREVIOUS);
    p.sond()
        .args(["template", "set"])
        .arg(fixture("templates/custom.md"))
        .assert()
        .success();
    assert_template(&p, &custom());
}

#[test]
fn later_edits_to_the_source_do_not_leak_into_the_template() {
    let p = Project::empty();
    let source = p.dir.join("mine.md");
    fs::write(&source, "v1\n").unwrap();
    p.sond()
        .args(["template", "set", "mine.md"])
        .assert()
        .success();
    fs::write(&source, "v2\n").unwrap();
    assert_template(&p, "v1\n");
}

#[test]
fn set_keeps_unknown_placeholders_and_odd_content_verbatim() {
    // Sond does not validate templates; an empty one is a valid choice too.
    for content in ["{{ status }} {{ title }}\n", "", "no placeholders at all"] {
        let p = Project::empty();
        fs::write(p.dir.join("t.md"), content).unwrap();
        p.sond()
            .args(["template", "set", "t.md"])
            .assert()
            .success();
        assert_template(&p, content);
    }
}

#[test]
fn setting_the_template_to_itself_is_harmless() {
    // The README example `sond template set ~/.config/sond/template.md`.
    let p = Project::empty();
    p.set_template(PREVIOUS);
    p.sond()
        .args(["template", "set"])
        .arg(p.template_file())
        .assert()
        .success();
    assert_template(&p, PREVIOUS);
}

#[test]
fn set_falls_back_to_home_dot_config() {
    let p = Project::empty();
    p.sond()
        .env_remove("XDG_CONFIG_HOME")
        .args(["template", "set"])
        .arg(fixture("templates/minimal.md"))
        .assert()
        .success();
    assert_eq!(
        fs::read(p.home.join(".config/sond/template.md")).unwrap(),
        fs::read(fixture("templates/minimal.md")).unwrap()
    );
    assert!(!p.template_file().exists());
}

#[test]
fn set_touches_neither_logs_nor_the_editor() {
    let p = Project::empty();
    p.sond()
        .args(["template", "set"])
        .arg(fixture("templates/custom.md"))
        .assert()
        .success();
    assert!(!p.logs_dir().exists());
    assert!(p.editor_invocations().is_empty());
}

// --- failures leave the previous template alone -----------------------------

#[test]
fn missing_source_is_an_error() {
    let p = Project::empty();
    p.set_template(PREVIOUS);
    p.sond()
        .args(["template", "set", "does-not-exist.md"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("does-not-exist.md"));
    assert_template(&p, PREVIOUS);
}

#[test]
fn missing_source_does_not_create_a_template() {
    let p = Project::empty();
    p.sond()
        .args(["template", "set", "does-not-exist.md"])
        .assert()
        .failure();
    assert!(!p.template_file().exists());
}

#[test]
fn directory_source_is_an_error() {
    let p = Project::empty();
    p.set_template(PREVIOUS);
    fs::create_dir(p.dir.join("a-dir")).unwrap();
    p.sond()
        .args(["template", "set", "a-dir"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("a-dir"));
    assert_template(&p, PREVIOUS);
}

#[test]
fn unreadable_source_is_an_error() {
    let p = Project::empty();
    if permissions_ignored(&p) {
        return;
    }
    p.set_template(PREVIOUS);
    let source = p.dir.join("secret.md");
    fs::write(&source, "# secret\n").unwrap();
    fs::set_permissions(&source, fs::Permissions::from_mode(0o000)).unwrap();

    p.sond()
        .args(["template", "set", "secret.md"])
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("secret.md").and(predicate::str::contains("panicked").not()),
        );
    assert_template(&p, PREVIOUS);
}

#[test]
fn non_utf8_source_is_an_error() {
    // `new` could not read it back, so refuse it now rather than then.
    let p = Project::empty();
    p.set_template(PREVIOUS);
    fs::write(p.dir.join("latin1.md"), b"# \xe9t\xe9 {{ title }}\n").unwrap();
    p.sond()
        .args(["template", "set", "latin1.md"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("UTF-8"));
    assert_template(&p, PREVIOUS);
}

#[test]
fn unwritable_config_is_an_error_and_leaves_no_debris() {
    let p = Project::empty();
    if permissions_ignored(&p) {
        return;
    }
    p.set_template(PREVIOUS);
    let dir = p.config.join("sond");
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o555)).unwrap();

    let assert = p
        .sond()
        .args(["template", "set"])
        .arg(fixture("templates/custom.md"))
        .assert();

    fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
    assert
        .failure()
        .stderr(predicate::str::contains("panicked").not());
    assert_template(&p, PREVIOUS);
}

#[test]
fn no_config_location_is_an_error() {
    let p = Project::empty();
    p.sond()
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("HOME")
        .args(["template", "set"])
        .arg(fixture("templates/custom.md"))
        .assert()
        .failure()
        .stderr(predicate::str::contains("HOME"));
}

// ---------------------------------------------------------------------------
// template edit
// ---------------------------------------------------------------------------

#[test]
fn edit_creates_the_default_template_when_there_is_none() {
    let p = Project::empty();
    p.sond()
        .args(["template", "edit"])
        .assert()
        .success()
        .stdout(format!("{}\n", p.template_file().display()));
    assert_template(&p, DEFAULT_TEMPLATE);
}

#[test]
fn edit_opens_the_template_in_the_editor() {
    let p = Project::empty();
    p.sond().args(["template", "edit"]).assert().success();
    assert_eq!(
        p.editor_invocations(),
        [[p.template_file().display().to_string()]]
    );
}

#[test]
fn edit_never_overwrites_an_existing_template() {
    let p = Project::empty();
    p.set_template(PREVIOUS);
    p.sond().args(["template", "edit"]).assert().success();
    assert_template(&p, PREVIOUS);
    assert_eq!(p.editor_invocations().len(), 1);
}

#[test]
fn edits_made_in_the_editor_shape_the_next_log() {
    let p = Project::empty();
    p.sond()
        .env("FAKE_EDITOR_APPEND", "## Hypothesis ({{ date }})")
        .args(["template", "edit"])
        .assert()
        .success();
    p.sond().args(["new", "Mesh refinement"]).assert().success();

    let log = p.read_log("R001-2026-09-21-mesh-refinement.md");
    assert!(log.starts_with("# Mesh refinement\n"), "{log}");
    assert!(
        log.ends_with("## Next steps\n## Hypothesis (2026-09-21)\n"),
        "{log}"
    );
}

#[test]
fn edit_falls_back_to_home_dot_config() {
    let p = Project::empty();
    let expected = p.home.join(".config/sond/template.md");
    p.sond()
        .env_remove("XDG_CONFIG_HOME")
        .args(["template", "edit"])
        .assert()
        .success();
    assert_eq!(fs::read_to_string(&expected).unwrap(), DEFAULT_TEMPLATE);
    assert_eq!(p.editor_invocations(), [[expected.display().to_string()]]);
}

#[test]
fn edit_does_not_touch_logs() {
    let p = Project::empty();
    p.sond().args(["template", "edit"]).assert().success();
    assert!(!p.logs_dir().exists());
}

#[test]
fn failing_editor_is_an_error_but_the_template_is_kept() {
    let p = Project::empty();
    p.sond()
        .env("EDITOR", sh_editor("editor/failing-editor.sh"))
        .args(["template", "edit"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("editor"));
    assert_template(&p, DEFAULT_TEMPLATE);
}

#[test]
fn template_path_that_is_a_directory_is_an_error() {
    let p = Project::empty();
    fs::create_dir_all(p.template_file()).unwrap();
    p.sond()
        .args(["template", "edit"])
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("template.md").and(predicate::str::contains("panicked").not()),
        );
    assert!(p.editor_invocations().is_empty());
}

#[test]
fn edit_without_a_config_location_is_an_error() {
    let p = Project::empty();
    p.sond()
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("HOME")
        .args(["template", "edit"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("HOME"));
    assert!(p.editor_invocations().is_empty());
}

// ---------------------------------------------------------------------------
// workflow
// ---------------------------------------------------------------------------

#[test]
fn set_then_edit_then_new() {
    let p = Project::empty();
    p.sond()
        .args(["template", "set"])
        .arg(fixture("templates/minimal.md"))
        .assert()
        .success();
    p.sond()
        .env("FAKE_EDITOR_APPEND", "## Sources")
        .args(["template", "edit"])
        .assert()
        .success();
    p.sond().args(["new", "CFL"]).assert().success();

    assert_eq!(
        p.read_log("R001-2026-09-21-cfl.md"),
        "# CFL\n\nCreated: 2026-09-21 12:19\nID: R001\n\n## Notes\n## Sources\n"
    );
}
