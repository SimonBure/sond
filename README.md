<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="assets/sond-header-dark.svg">
    <img src="assets/sond-header.svg" alt="sond" width="239" height="80">
  </picture>
</p>

Append-only research logs, stored as Markdown next to your code or your project.

The name is the French *sonder*, to probe or to sound out, cut short.

A Sond log represents one investigation. `new` starts one, `poke` continues
it, `search` finds past investigations, `ask` finds them by meaning (optional,
see [Semantic search](#semantic-search-optional)), and `recent` shows what you
have been working on. Logs are ordinary Markdown files in `./sond/`: no
database, no Git requirement, readable and searchable with any editor, `grep`,
or `rg`.

## Install

```sh
cargo install --path .
```

### Set up your editor (optional)

`sond new` and `sond poke` only write the file and print its path. To open
it in your editor as well, pass `-e` / `--edit`. Sond then uses the editor
named in `$VISUAL` or `$EDITOR`, so set one once in your shell's startup
file (`~/.bashrc`, `~/.zshrc`, …):

```sh
export EDITOR="zed --wait"      # Zed
export EDITOR="code --wait"     # VS Code
export EDITOR="nvim"            # terminal editors need no flag
```

Open a new terminal (or run `source ~/.bashrc`) for it to take effect. Then
`sond new -e <title>`:

1. creates `sond/R<id>-<date>-<slug>.md` from the template,
2. prints its path,
3. runs `$EDITOR <path>` and waits for it to exit,
4. exits with an error if the editor fails, but keeps the log.

With `--wait`, a GUI editor blocks until you close the tab or window, so
`sond -e` returns then. Sond itself does not need it: plain `EDITOR="zed"`
opens the file and returns at once. But `$EDITOR` is shared with Git,
`crontab -e` and others, which read the file back after the editor exits
(`git commit` without `-m` aborts on an empty message otherwise), so keep
`--wait`.

`sond poke -e <id>` works the same way. VS Code and terminal editors open at
the new dated section; Zed opens the file at the top.

Without `$VISUAL` or `$EDITOR`, `-e` and `template edit` fall back to `vi`
(`Esc`, then `:q!` to leave it). Plain `new` and `poke` never open anything.

### Semantic search (optional)

`sond ask` finds the sections closest in meaning to a question, in any
language the logs are written in. It runs an embedding model locally, so it
is a build feature, off by default:

```sh
cargo install sond --features ask          # from crates.io
cargo install --path . --features ask      # from a clone
```

The binary grows from about 2 MB to about 40 MB (ONNX Runtime is linked in).
Not available on Intel Macs, for which ONNX Runtime ships no prebuilt library.
Without the feature, `index` and `ask` say how to get it.

Run `sond index` once per project, in a terminal: the first time, it asks
before downloading the model (EmbeddingGemma 300M, 4-bit, about 200 MB) into
`$XDG_CACHE_HOME/sond` (default `~/.cache/sond`), shared by every project.
If `HF_HOME` is set, the model goes there instead.

## Usage

```sh
sond new Adaptive timestep instability   # quotes optional
# sond/R001-2026-09-21-adaptive-timestep-instability.md
sond new -e Mesh refinement              # -e / --edit: also open it in $EDITOR

sond poke R001                           # also R1, r001, 1
# appends a dated section to the same file; -e opens it there

sond search CFL instability              # quotes optional
# R001  Adaptive timestep instability
#   12: Unstable once the CFL instability kicks in at dt > 0.01.

sond index                               # once: builds sond/.index
sond ask why does the solver blow up     # quotes optional; 5 hits by default
# R001  Adaptive timestep instability
#   L10  ## Investigation  0.62
sond ask -n 10 which datasets did we consider

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
- **ask** searches `sond/.index`: one vector per `##` section (long sections
  are split at paragraphs; text before the first `##` is left out). It
  updates the index itself before answering, embedding only the sections that
  are new or changed, so `sond index` is needed only once. Creating the index
  adds `sond/.index` to `./.gitignore`, if there is one.
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

- **Editor**: only used by `new -e`, `poke -e` and `template edit`; see
  [Set up your editor](#set-up-your-editor-optional). `$VISUAL`, then
  `$EDITOR`, then `vi`. Flags are split on whitespace, so an editor path
  containing spaces does not work. `poke -e` opens vim, nvim, nano, emacs,
  VS Code and helix at the new section.

## Development

```sh
cargo test
cargo test --features ask                        # semantic search too
cargo test --features ask -- --include-ignored   # and those needing the model
```

The tests needing the model use the one installed in your cache (run
`sond index` once first). `examples/eval.rs` compares embedding models on real logs, from a
file of queries and the sections they should find; EmbeddingGemma was chosen
with it (`cargo run --release --features ask --example eval -- queries.tsv`).

Tests never touch your real `$HOME`, editor, or clock: each runs in a temporary
project with a fake editor and a fixed time (`SOND_NOW="YYYY-MM-DD HH:MM"`).
Tests of the "no `$EDITOR` set" case also shadow `vi`, `vim` and `nano` on
`$PATH` with fakes, so a regression fails fast instead of opening a real `vi`.
