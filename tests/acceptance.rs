//! The v1 definition of done: one investigation, start to finish.

#![cfg(unix)]

mod common;

use std::fs;

use common::{Project, fixture};

#[test]
fn a_research_week_in_one_investigation() {
    let p = Project::empty();

    // 1–2. An empty project; configure a custom template.
    p.probe()
        .args(["template", "set"])
        .arg(fixture("templates/minimal.md"))
        .assert()
        .success();

    // 3–4. Start an investigation and write initial observations.
    p.probe()
        .env("PROBE_NOW", "2026-09-21 12:19")
        .env("FAKE_EDITOR_APPEND", "Unstable once dt > 0.01.")
        .args(["new", "Adaptive timestep instability"])
        .assert()
        .success();

    // 5–6. A day later, continue it.
    p.probe()
        .env("PROBE_NOW", "2026-09-22 09:40")
        .env(
            "FAKE_EDITOR_APPEND",
            "The CFL number exceeds 1 exactly there.",
        )
        .args(["poke", "R001"])
        .assert()
        .success();

    // 7. Find it again by a concept.
    p.probe()
        .args(["search", "cfl number"])
        .assert()
        .success()
        .stdout(
            "R001  Adaptive timestep instability\n  \
             13: The CFL number exceeds 1 exactly there.\n",
        );

    // 8. See it in recent activity.
    p.probe()
        .arg("recent")
        .assert()
        .success()
        .stdout("R001  2026-09-22 09:40  Adaptive timestep instability\n");

    // One ordinary Markdown file holding the whole dated history, and nothing
    // else in the project: no database, no index, no hidden state.
    let name = "R001-2026-09-21-adaptive-timestep-instability.md";
    assert_eq!(p.log_names(), [name]);
    assert_eq!(
        fs::read_dir(&p.dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect::<Vec<_>>(),
        ["logs"]
    );
    assert_eq!(
        p.read_log(name),
        "\
# Adaptive timestep instability

Created: 2026-09-21 12:19
ID: R001

## Notes
Unstable once dt > 0.01.

---

## 2026-09-22 09:40

The CFL number exceeds 1 exactly there.
"
    );
}
