//! M1 — pure functions, no I/O.
//!
//! These tests are the specification for milestone 1. Every function here is
//! deterministic: no filesystem, no clock, no environment. That is what makes
//! them exhaustively testable, and it is why they are worth writing first.
//!
//! To make these compile, create:
//!
//! ```text
//! src/lib.rs      pub mod editor;  pub mod log;  pub mod template;
//! src/log.rs      slugify, format_id, parse_id, log_filename,
//!                 parse_log_filename, LogFile, trailing_empty_section_line
//! src/template.rs DEFAULT_TEMPLATE, TemplateVars, render_template
//! src/editor.rs   EditorCommand, editor_command
//! ```
//!
//! Run just this file with `cargo test --test pure`.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use probe::editor::{choose_editor, editor_command};
use probe::log::{
    format_id, log_filename, parse_id, parse_log_filename, slugify, trailing_empty_section_line,
};
use probe::template::{DEFAULT_TEMPLATE, TemplateVars, render_template, template_path_from};

// ---------------------------------------------------------------------------
// slugify
// ---------------------------------------------------------------------------

#[test]
fn slugify_lowercases_and_hyphenates() {
    assert_eq!(
        slugify("Adaptive timestep instability"),
        "adaptive-timestep-instability"
    );
}

#[test]
fn slugify_folds_accents_to_ascii() {
    // Non-negotiable: research titles will be written in French.
    assert_eq!(slugify("Précision numérique"), "precision-numerique");
    assert_eq!(slugify("Élément fini"), "element-fini");
    assert_eq!(slugify("Cœur de la boucle"), "coeur-de-la-boucle");
    assert_eq!(slugify("Où ça"), "ou-ca");
    assert_eq!(slugify("Bœuf à l'unité"), "boeuf-a-l-unite");
}

#[test]
fn slugify_drops_punctuation() {
    assert_eq!(slugify("CFL condition!"), "cfl-condition");
    assert_eq!(slugify("Why does it diverge?"), "why-does-it-diverge");
    assert_eq!(slugify("Smith et al. (2024)"), "smith-et-al-2024");
    assert_eq!(slugify("high/low resolution"), "high-low-resolution");
    assert_eq!(slugify("a_b_c"), "a-b-c");
}

#[test]
fn slugify_collapses_and_trims_separators() {
    assert_eq!(slugify("  spaced   out  "), "spaced-out");
    assert_eq!(slugify("--leading and trailing--"), "leading-and-trailing");
    assert_eq!(slugify("a  --  b"), "a-b");
}

#[test]
fn slugify_keeps_digits() {
    assert_eq!(slugify("Run 42 diverged"), "run-42-diverged");
    assert_eq!(slugify("2026 roadmap"), "2026-roadmap");
}

#[test]
fn slugify_falls_back_to_untitled_when_nothing_survives() {
    // A slug is part of a filename, so it can never be empty.
    assert_eq!(slugify(""), "untitled");
    assert_eq!(slugify("???"), "untitled");
    assert_eq!(slugify("   "), "untitled");
    // Scripts we cannot fold to ASCII are dropped rather than mangled.
    assert_eq!(slugify("日本語"), "untitled");
}

#[test]
fn slugify_truncates_at_a_word_boundary() {
    let long = "This is an extremely long research log title that certainly exceeds the sixty character limit";
    let s = slugify(long);

    // Exact expected value, so the truncation rule is unambiguous:
    // cut at the last '-' that keeps the slug within 60 characters.
    assert_eq!(
        s,
        "this-is-an-extremely-long-research-log-title-that-certainly"
    );

    // ...and the properties that rule must always satisfy.
    assert!(s.len() <= 60, "slug was {} chars: {s}", s.len());
    assert!(!s.ends_with('-'), "slug must not end with a separator");
    assert!(!s.starts_with('-'), "slug must not start with a separator");
}

#[test]
fn slugify_output_is_always_filesystem_safe() {
    let nasty = "../../etc/passwd: \"quoted\" <redirect> | pipe * star \\ back";
    let s = slugify(nasty);
    assert!(
        s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
        "slug contained unsafe characters: {s}"
    );
    assert!(
        !s.contains(".."),
        "slug must never allow path traversal: {s}"
    );
}

// ---------------------------------------------------------------------------
// IDs
// ---------------------------------------------------------------------------

#[test]
fn format_id_pads_to_three_digits_then_widens() {
    assert_eq!(format_id(1), "R001");
    assert_eq!(format_id(42), "R042");
    assert_eq!(format_id(999), "R999");
    // Past 999 we simply get wider. No renumbering, no reformatting.
    assert_eq!(format_id(1000), "R1000");
    assert_eq!(format_id(12345), "R12345");
}

#[test]
fn parse_id_is_tolerant_of_what_a_human_types() {
    // `probe poke` should accept every reasonable spelling of the same log.
    assert_eq!(parse_id("R042"), Some(42));
    assert_eq!(parse_id("R42"), Some(42));
    assert_eq!(parse_id("r042"), Some(42));
    assert_eq!(parse_id("42"), Some(42));
    assert_eq!(parse_id("  R042  "), Some(42));
    assert_eq!(parse_id("R0042"), Some(42));
}

#[test]
fn parse_id_rejects_nonsense() {
    assert_eq!(parse_id(""), None);
    assert_eq!(parse_id("R"), None);
    assert_eq!(parse_id("Rabc"), None);
    assert_eq!(parse_id("R-1"), None);
    assert_eq!(parse_id("R4 2"), None);
    assert_eq!(parse_id("42R"), None);
    assert_eq!(parse_id("R042.md"), None);
}

#[test]
fn format_and_parse_id_round_trip() {
    for n in [1u32, 7, 42, 99, 100, 999, 1000, 54321] {
        assert_eq!(
            parse_id(&format_id(n)),
            Some(n),
            "round trip failed for {n}"
        );
    }
}

// ---------------------------------------------------------------------------
// filenames
// ---------------------------------------------------------------------------

#[test]
fn log_filename_is_deterministic_and_sortable() {
    assert_eq!(
        log_filename(42, "2026-08-19", "adaptive-timestep-instability"),
        "R042-2026-08-19-adaptive-timestep-instability.md"
    );
    assert_eq!(
        log_filename(1, "2026-01-01", "untitled"),
        "R001-2026-01-01-untitled.md"
    );
}

#[test]
fn parse_log_filename_reads_back_what_we_wrote() {
    let f = parse_log_filename("R042-2026-08-19-adaptive-timestep-instability.md").unwrap();
    assert_eq!(f.id, 42);
    assert_eq!(f.date.as_deref(), Some("2026-08-19"));
    assert_eq!(f.slug, "adaptive-timestep-instability");
}

#[test]
fn parse_log_filename_tolerates_a_renamed_slug() {
    // The filename is the index, but only the `R<id>-` prefix is load-bearing.
    // Renaming the descriptive part must never orphan a log.
    let f = parse_log_filename("R042-2026-08-19-totally-different-words.md").unwrap();
    assert_eq!(f.id, 42);
    assert_eq!(f.slug, "totally-different-words");
}

#[test]
fn parse_log_filename_tolerates_a_missing_date() {
    // A user who renames to `R042-notes.md` should still be able to poke R042.
    let f = parse_log_filename("R042-notes.md").unwrap();
    assert_eq!(f.id, 42);
    assert_eq!(f.date, None);
    assert_eq!(f.slug, "notes");
}

#[test]
fn parse_log_filename_rejects_non_logs() {
    assert!(parse_log_filename("README.md").is_none());
    assert!(parse_log_filename("notes.md").is_none());
    assert!(parse_log_filename("R042").is_none(), "must be .md");
    assert!(parse_log_filename("R042-2026-08-19-x.txt").is_none());
    assert!(parse_log_filename("Rxyz-2026-08-19-x.md").is_none());
    assert!(parse_log_filename(".hidden.md").is_none());
    // No `-` after the id means there is no slug at all.
    assert!(parse_log_filename("R042.md").is_none());
}

#[test]
fn filename_round_trips_through_the_parser() {
    let name = log_filename(7, "2026-12-31", "new-year-instability");
    let f = parse_log_filename(&name).unwrap();
    assert_eq!(f.id, 7);
    assert_eq!(f.date.as_deref(), Some("2026-12-31"));
    assert_eq!(f.slug, "new-year-instability");
}

// ---------------------------------------------------------------------------
// template rendering
// ---------------------------------------------------------------------------

fn vars() -> TemplateVars<'static> {
    TemplateVars {
        title: "Adaptive timestep instability",
        id: "R042",
        created: "2026-08-19 09:14",
        date: "2026-08-19",
    }
}

#[test]
fn render_substitutes_the_four_known_variables() {
    let out = render_template(
        "# {{ title }}\n\nCreated: {{ created }}\nID: {{ id }}\nDate: {{ date }}\n",
        &vars(),
    );
    assert_eq!(
        out,
        "# Adaptive timestep instability\n\nCreated: 2026-08-19 09:14\nID: R042\nDate: 2026-08-19\n"
    );
}

#[test]
fn render_is_tolerant_of_inner_whitespace() {
    assert_eq!(render_template("{{title}}", &vars()), vars().title);
    assert_eq!(render_template("{{ title }}", &vars()), vars().title);
    assert_eq!(render_template("{{   title   }}", &vars()), vars().title);
    assert_eq!(render_template("{{\ttitle\t}}", &vars()), vars().title);
}

#[test]
fn render_replaces_every_occurrence() {
    assert_eq!(
        render_template("{{ id }} {{ id }} {{ id }}", &vars()),
        "R042 R042 R042"
    );
}

#[test]
fn render_leaves_unknown_placeholders_untouched() {
    // The template is the user's file, not a schema we validate. Probe
    // substitutes only what it owns and never errors on the rest — that is
    // what lets a researcher use any template structure they like.
    assert_eq!(
        render_template("{{ status }} and {{ tags }}", &vars()),
        "{{ status }} and {{ tags }}"
    );
    assert_eq!(render_template("{{ titel }}", &vars()), "{{ titel }}");
}

#[test]
fn render_leaves_malformed_delimiters_untouched() {
    assert_eq!(render_template("{{ title", &vars()), "{{ title");
    assert_eq!(render_template("title }}", &vars()), "title }}");
    assert_eq!(render_template("{ title }", &vars()), "{ title }");
    assert_eq!(render_template("}}{{", &vars()), "}}{{");
}

#[test]
fn render_passes_through_content_with_no_placeholders() {
    let md = "# Notes\n\nSome $math$ and a {brace} and 100%\n";
    assert_eq!(render_template(md, &vars()), md);
    assert_eq!(render_template("", &vars()), "");
}

#[test]
fn default_template_renders_without_leftover_placeholders() {
    let out = render_template(DEFAULT_TEMPLATE, &vars());
    assert!(
        !out.contains("{{"),
        "default template left an unsubstituted placeholder:\n{out}"
    );
    assert!(out.contains("Adaptive timestep instability"));
    assert!(out.contains("R042"));
    assert!(
        out.starts_with("# "),
        "template should open with the title heading"
    );
}

// ---------------------------------------------------------------------------
// editor invocation
// ---------------------------------------------------------------------------

const P: &str = "/proj/logs/R042-2026-08-19-adaptive.md";

fn path() -> &'static Path {
    Path::new(P)
}

#[test]
fn terminal_editors_take_plus_line() {
    for prog in ["vi", "vim", "nvim", "nano", "emacs"] {
        let c = editor_command(prog, path(), Some(12)).unwrap();
        assert_eq!(c.program, prog);
        assert_eq!(c.args, vec!["+12".to_string(), P.to_string()], "for {prog}");
    }
}

#[test]
fn no_line_means_just_the_path() {
    let c = editor_command("vim", path(), None).unwrap();
    assert_eq!(c.program, "vim");
    assert_eq!(c.args, vec![P.to_string()]);
}

#[test]
fn vscode_uses_goto_and_keeps_its_flags() {
    // `EDITOR="code --wait"` is explicitly required to work.
    let c = editor_command("code --wait", path(), Some(12)).unwrap();
    assert_eq!(c.program, "code");
    assert_eq!(
        c.args,
        vec![
            "--wait".to_string(),
            "--goto".to_string(),
            format!("{P}:12")
        ]
    );

    let c = editor_command("code --wait", path(), None).unwrap();
    assert_eq!(c.program, "code");
    assert_eq!(c.args, vec!["--wait".to_string(), P.to_string()]);
}

#[test]
fn helix_uses_path_colon_line() {
    for prog in ["hx", "helix"] {
        let c = editor_command(prog, path(), Some(12)).unwrap();
        assert_eq!(c.program, prog);
        assert_eq!(c.args, vec![format!("{P}:12")], "for {prog}");
    }
}

#[test]
fn editor_is_recognised_by_basename_not_full_path() {
    let c = editor_command("/usr/bin/vim", path(), Some(12)).unwrap();
    assert_eq!(c.program, "/usr/bin/vim");
    assert_eq!(c.args, vec!["+12".to_string(), P.to_string()]);
}

#[test]
fn unknown_editors_get_the_bare_path() {
    // We must never guess a line-number syntax we do not know: a wrong guess
    // turns into a mystery argument the editor may treat as a file to create.
    let c = editor_command("subl", path(), Some(12)).unwrap();
    assert_eq!(c.program, "subl");
    assert_eq!(c.args, vec![P.to_string()]);

    let c = editor_command("myeditor --flag", path(), Some(12)).unwrap();
    assert_eq!(c.program, "myeditor");
    assert_eq!(c.args, vec!["--flag".to_string(), P.to_string()]);
}

#[test]
fn blank_editor_yields_nothing_to_run() {
    assert!(editor_command("", path(), None).is_none());
    assert!(editor_command("   ", path(), Some(3)).is_none());
}

#[test]
fn visual_wins_over_editor() {
    assert_eq!(
        choose_editor(Some("code --wait"), Some("vim")),
        "code --wait"
    );
}

#[test]
fn editor_is_used_when_visual_is_unset_or_blank() {
    assert_eq!(choose_editor(None, Some("vim")), "vim");
    assert_eq!(choose_editor(Some(""), Some("vim")), "vim");
    assert_eq!(choose_editor(Some("  "), Some("vim")), "vim");
}

#[test]
fn vi_is_the_last_resort() {
    // The Unix convention, same as git and crontab.
    assert_eq!(choose_editor(None, None), "vi");
    assert_eq!(choose_editor(Some(""), Some(" ")), "vi");
}

// ---------------------------------------------------------------------------
// template location
// ---------------------------------------------------------------------------

fn os(s: &str) -> Option<OsString> {
    Some(OsString::from(s))
}

#[test]
fn template_lives_under_xdg_config_home() {
    assert_eq!(
        template_path_from(os("/xdg"), os("/home/me")),
        Some(PathBuf::from("/xdg/probe/template.md"))
    );
}

#[test]
fn template_falls_back_to_dot_config() {
    assert_eq!(
        template_path_from(None, os("/home/me")),
        Some(PathBuf::from("/home/me/.config/probe/template.md"))
    );
}

#[test]
fn empty_or_relative_xdg_config_home_is_ignored() {
    // Per the XDG Base Directory spec.
    let expected = Some(PathBuf::from("/home/me/.config/probe/template.md"));
    assert_eq!(template_path_from(os(""), os("/home/me")), expected);
    assert_eq!(template_path_from(os("rel/dir"), os("/home/me")), expected);
}

#[test]
fn no_config_location_means_no_template_path() {
    assert_eq!(template_path_from(None, None), None);
    assert_eq!(template_path_from(os(""), os("")), None);
}

// ---------------------------------------------------------------------------
// trailing empty section (poke idempotency)
// ---------------------------------------------------------------------------
//
// Returns the 1-based line number of the last `## YYYY-MM-DD HH:MM` heading
// when everything after it is whitespace. `poke` uses this to reopen an
// abandoned section instead of stacking up empty stubs.

#[test]
fn detects_an_empty_trailing_section() {
    let c = "# Title\n\n## 2026-08-19 14:32\n\n";
    assert_eq!(trailing_empty_section_line(c), Some(3));
}

#[test]
fn detects_an_empty_trailing_section_without_final_newline() {
    let c = "# Title\n\n## 2026-08-19 14:32";
    assert_eq!(trailing_empty_section_line(c), Some(3));
}

#[test]
fn ignores_a_section_that_has_content() {
    let c = "# Title\n\n## 2026-08-19 14:32\n\nTried a fixed timestep.\n";
    assert_eq!(trailing_empty_section_line(c), None);
}

#[test]
fn finds_only_the_last_section() {
    let c = "\
# Title

## 2026-08-19 14:32

Tried a fixed timestep.

---

## 2026-08-20 09:05

";
    assert_eq!(trailing_empty_section_line(c), Some(9));
}

#[test]
fn returns_none_when_the_last_section_is_written_in() {
    let c = "\
# Title

## 2026-08-19 14:32

Nothing yet.

---

## 2026-08-20 09:05

Found it.
";
    assert_eq!(trailing_empty_section_line(c), None);
}

#[test]
fn ordinary_headings_are_not_dated_sections() {
    // A fresh log from the default template ends in empty `## Next steps`.
    // That must NOT be mistaken for an abandoned poke.
    assert_eq!(
        trailing_empty_section_line("# T\n\n## Next steps\n\n"),
        None
    );
    assert_eq!(trailing_empty_section_line("# T\n\n## Context\n"), None);
    assert_eq!(
        trailing_empty_section_line("# T\n\n# 2026-08-19 14:32\n\n"),
        None
    );
    assert_eq!(
        trailing_empty_section_line("# T\n\n### 2026-08-19 14:32\n\n"),
        None
    );
}

#[test]
fn malformed_dates_are_not_sections() {
    assert_eq!(trailing_empty_section_line("## 2026-8-19 14:32\n\n"), None);
    assert_eq!(trailing_empty_section_line("## 2026-08-19\n\n"), None);
    assert_eq!(trailing_empty_section_line("## 26-08-19 14:32\n\n"), None);
    assert_eq!(trailing_empty_section_line("## 2026-08-19 4:32\n\n"), None);
}

#[test]
fn handles_files_with_no_sections_at_all() {
    assert_eq!(trailing_empty_section_line(""), None);
    assert_eq!(trailing_empty_section_line("\n\n\n"), None);
    assert_eq!(trailing_empty_section_line("# Just a title\n"), None);
}

#[test]
fn a_trailing_separator_still_counts_as_empty() {
    // `poke` writes `\n---\n\n## <date>\n\n`; whitespace-only tails are empty,
    // but a `---` *after* the heading is content the user chose to leave.
    let c = "# T\n\n## 2026-08-19 14:32\n   \n\t\n";
    assert_eq!(trailing_empty_section_line(c), Some(3));
}
