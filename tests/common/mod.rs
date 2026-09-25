//! A throwaway Sond project for CLI integration tests.
//!
//! Every command runs with a cleared environment: its own `$HOME`, its own
//! `$XDG_CONFIG_HOME`, a fixed clock (`SOND_NOW`), and a fake editor that
//! records how it was called instead of opening a window. Nothing a test does
//! can see or touch the real user's config, editor, clock, or Git repository.
//!
//! ```text
//! <tempdir>/
//! ├── project/            cwd of every command; sond/ goes here
//! ├── home/               $HOME
//! ├── config/             $XDG_CONFIG_HOME
//! └── editor.log          one line per editor invocation, args tab-separated
//! ```

#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use tempfile::TempDir;

/// The fixed "now" every command sees unless a test overrides `SOND_NOW`.
pub const NOW: &str = "2026-09-21 12:19";

pub fn fixture(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(rel)
}

/// `$EDITOR` value that runs a fixture script through `/bin/sh`.
pub fn sh_editor(script: &str) -> String {
    format!("/bin/sh {}", fixture(script).display())
}

pub struct Project {
    _root: TempDir,
    pub dir: PathBuf,
    pub home: PathBuf,
    pub config: PathBuf,
    editor_log: PathBuf,
}

impl Project {
    /// A project directory with nothing in it, not even `sond/`.
    pub fn empty() -> Self {
        let root = tempfile::tempdir().expect("create tempdir");
        let dir = root.path().join("project");
        let home = root.path().join("home");
        let config = root.path().join("config");
        for d in [&dir, &home, &config] {
            fs::create_dir(d).expect("create fixture dir");
        }
        Self {
            editor_log: root.path().join("editor.log"),
            _root: root,
            dir,
            home,
            config,
        }
    }

    /// A project whose `sond/` holds copies of these `tests/fixtures/logs` files.
    pub fn with_fixture_logs(names: &[&str]) -> Self {
        let p = Self::empty();
        for name in names {
            p.add_log(
                name,
                &fs::read_to_string(fixture(&format!("logs/{name}"))).unwrap(),
            );
        }
        p
    }

    pub fn logs_dir(&self) -> PathBuf {
        self.dir.join("sond")
    }

    pub fn add_log(&self, name: &str, content: &str) {
        fs::create_dir_all(self.logs_dir()).unwrap();
        fs::write(self.logs_dir().join(name), content).unwrap();
    }

    /// Where Sond keeps the user's template in this project's environment.
    pub fn template_file(&self) -> PathBuf {
        self.config.join("sond/template.md")
    }

    /// Installs `content` as the user's template.
    pub fn set_template(&self, content: &str) {
        fs::create_dir_all(self.config.join("sond")).unwrap();
        fs::write(self.template_file(), content).unwrap();
    }

    /// Names of the entries in the template's directory, sorted.
    pub fn config_names(&self) -> Vec<String> {
        let Ok(entries) = fs::read_dir(self.config.join("sond")) else {
            return Vec::new();
        };
        let mut names: Vec<String> = entries
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }

    /// `sond` with a hermetic environment, run from the project directory.
    pub fn sond(&self) -> Command {
        let mut cmd = assert_cmd::cargo::cargo_bin_cmd!("sond");
        cmd.current_dir(&self.dir)
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", &self.config)
            .env("SOND_NOW", NOW)
            .env("EDITOR", sh_editor("editor/fake-editor.sh"))
            .env("FAKE_EDITOR_LOG", &self.editor_log);
        cmd
    }

    /// `sond` as run by a user who never set `$VISUAL` or `$EDITOR`. The
    /// usual fallbacks (`vi`, `vim`, `nano`) are shadowed on `$PATH` by fakes
    /// that record their call, so a regression shows up in
    /// [`Self::editor_invocations`] instead of hanging the test on a real `vi`.
    pub fn sond_without_editor(&self) -> Command {
        let mut path = std::ffi::OsString::from(fixture("editor/fallback"));
        path.push(":");
        path.push(std::env::var_os("PATH").unwrap_or_default());
        let mut cmd = self.sond();
        cmd.env_remove("EDITOR").env("PATH", path);
        cmd
    }

    /// Names of the files in `sond/`, sorted. Empty if `sond/` does not exist.
    pub fn log_names(&self) -> Vec<String> {
        let Ok(entries) = fs::read_dir(self.logs_dir()) else {
            return Vec::new();
        };
        let mut names: Vec<String> = entries
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }

    pub fn read_log(&self, name: &str) -> String {
        fs::read_to_string(self.logs_dir().join(name)).unwrap()
    }

    /// The arguments of each fake-editor invocation, in order.
    pub fn editor_invocations(&self) -> Vec<Vec<String>> {
        let Ok(log) = fs::read_to_string(&self.editor_log) else {
            return Vec::new();
        };
        log.lines()
            .map(|l| match l {
                "" => Vec::new(),
                _ => l.split('\t').map(str::to_string).collect(),
            })
            .collect()
    }
}
