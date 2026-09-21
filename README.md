# Probe

Append-only research logs, stored as Markdown next to your code.

A Probe log represents one investigation. `new` starts one, `poke` continues
it, `search` finds past investigations, and `recent` shows what you have been
working on. Logs are ordinary
Markdown files in `./logs/`: no database, no Git requirement, readable and
searchable with any editor, `grep`, or `rg`.

## Install

```sh
cargo install --path .
```

## Usage

```sh
probe new Adaptive timestep instability   # quotes optional
# logs/R001-2026-09-21-adaptive-timestep-instability.md, opened in $EDITOR

probe poke R001                           # also R1, r001, 1
# appends a dated section and reopens the same file

probe search CFL instability              # quotes optional
# R001  Adaptive timestep instability
#   12: Unstable once the CFL instability kicks in at dt > 0.01.

probe recent                              # newest first, 10 by default
# R001  2026-09-22 08:30  Adaptive timestep instability

probe recent -n 3

probe template edit                       # starts from the default template
probe template set ~/notes/template.md    # installs a copy
```

`new`, `poke` and `template` print the path of the file they touch. Probe runs
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
- **Templates** live at `$XDG_CONFIG_HOME/probe/template.md` (default
  `~/.config/probe/template.md`). Probe fills in `{{ title }}`, `{{ id }}`,
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
project with a fake editor and a fixed time (`PROBE_NOW="YYYY-MM-DD HH:MM"`).
