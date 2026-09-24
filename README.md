<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="assets/sond-header-dark.svg">
    <img src="assets/sond-header.svg" alt="sond" width="239" height="80">
  </picture>
</p>

Append-only research logs, stored as Markdown next to your code or your project.

The name is the French *sonder*, to probe or to sound out, cut short.

A Sond log represents one investigation. `new` starts one, `poke` continues
it, `search` finds past investigations, and `recent` shows what you have been
working on. Logs are ordinary Markdown files in `./logs/`: no database, no Git
requirement, readable and searchable with any editor, `grep`, or `rg`.

## Install

```sh
cargo install --path .
```

## Usage

```sh
sond new Adaptive timestep instability   # quotes optional
# logs/R001-2026-09-21-adaptive-timestep-instability.md, opened in $EDITOR

sond poke R001                           # also R1, r001, 1
# appends a dated section and reopens the same file

sond search CFL instability              # quotes optional
# R001  Adaptive timestep instability
#   12: Unstable once the CFL instability kicks in at dt > 0.01.

sond recent                              # newest first, 10 by default
# R001  2026-09-22 08:30  Adaptive timestep instability

sond recent -n 3

sond template edit                       # starts from the default template
sond template set ~/notes/template.md    # installs a copy
```

`new`, `poke` and `template` print the path of the file they touch. Sond runs
relative to the current directory.

## How it works

- **IDs**: `R001`, `R002`, … one more than the highest existing ID. Only the
  `R<id>-` filename prefix matters: rename the rest freely.
- **poke** appends and never rewrites:

  ```markdown
  ---

  ## 2026-09-22 08:30

  ```

  If the last section is an empty dated one, it is reopened instead of adding
  another.
- **search** is literal and case-insensitive: `a.b*` means those four
  characters. Results are grouped by log, most recently active first, with
  line numbers. It exits 1 when nothing matches, like `grep`.
- **recent** orders logs by the latest of their `Created:` line, dated
  sections, and filename date. File modification times are ignored.
- **Templates** live at `$XDG_CONFIG_HOME/sond/template.md` (default
  `~/.config/sond/template.md`). Sond fills in `{{ title }}`, `{{ id }}`,
  `{{ created }}` (`YYYY-MM-DD HH:MM`) and `{{ date }}`; any other `{{ … }}` is
  left untouched. Without a template, this is used:

  ```markdown
  # {{ title }}

  Created: {{ created }}
  ID: {{ id }}

  ## Context

  ## Investigation

  ## Next steps
  ```

- **Editor**: `$VISUAL`, then `$EDITOR`, then `vi`. Flags work
  (`EDITOR="code --wait"`); paths containing spaces do not. `poke` opens
  vim, nvim, nano, emacs, VS Code and helix at the new section.

## Development

```sh
cargo test
```

Tests never touch your real `$HOME`, editor, or clock: each runs in a temporary
project with a fake editor and a fixed time (`SOND_NOW="YYYY-MM-DD HH:MM"`).
