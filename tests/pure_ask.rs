//! Pure functions behind `sond index` and `sond ask`: no model, no I/O.
//!
//! - chunking: one chunk per non-empty `##` section, prefixed with the log
//!   title and the section heading; text before the first `##` is ignored;
//!   long sections are split at paragraph boundaries
//! - refresh: only chunks whose text is new are embedded; the rest keep
//!   their vector; a change of model re-embeds everything
//! - ranking: cosine similarity, best first
//! - the index file round-trips, and a damaged one is an error
//! - `.gitignore` gains `sond/.index` once
//! - the model lives in a per-user cache, found like the template
//!
//! To make these compile, create:
//!
//! ```text
//! src/chunk.rs    Chunk, chunks, MAX_WORDS
//! src/index.rs    Entry, Index, refresh, top_k, encode, decode,
//!                 with_index_ignored, INDEX_FILE, model_cache_dir_from
//! ```
//!
//! Run just this file with `cargo test --test pure_ask`.

use std::cell::RefCell;
use std::ffi::OsString;
use std::path::PathBuf;

use anyhow::Result;
use sond::chunk::{Chunk, MAX_WORDS, chunks};
use sond::index::{
    Entry, Index, decode, encode, model_cache_dir_from, refresh, top_k, with_index_ignored,
};

// ---------------------------------------------------------------------------
// chunking
// ---------------------------------------------------------------------------

const LOG: &str = "\
# Adaptive timestep instability

Created: 2026-09-20 10:15
ID: R003

## Context

The simulation becomes unstable
when the timestep is increased.

## Investigation

Initial experiments point at the CFL condition.
";

fn chunk(line: usize, heading: &str, text: &str) -> Chunk {
    Chunk {
        line,
        heading: heading.to_string(),
        text: text.to_string(),
    }
}

#[test]
fn one_chunk_per_section_with_title_and_heading() {
    assert_eq!(
        chunks("Adaptive timestep instability", LOG),
        vec![
            chunk(
                6,
                "Context",
                "# Adaptive timestep instability\n## Context\n\n\
                 The simulation becomes unstable\nwhen the timestep is increased."
            ),
            chunk(
                11,
                "Investigation",
                "# Adaptive timestep instability\n## Investigation\n\n\
                 Initial experiments point at the CFL condition."
            ),
        ]
    );
}

#[test]
fn text_before_the_first_section_is_ignored() {
    for c in chunks("T", LOG) {
        assert!(!c.text.contains("Created:"), "{}", c.text);
        assert!(!c.text.contains("ID: R003"), "{}", c.text);
    }
}

#[test]
fn a_log_without_sections_has_no_chunks() {
    assert_eq!(chunks("T", "# T\n\nJust a preamble.\n"), vec![]);
}

#[test]
fn empty_sections_are_ignored() {
    let log = "# T\n\n## Context\n\n   \n## Empty too\n## Real\n\nBody.\n\n## 2026-09-21 12:19\n\n";
    assert_eq!(
        chunks("T", log),
        vec![chunk(7, "Real", "# T\n## Real\n\nBody.")]
    );
}

#[test]
fn horizontal_rules_are_not_content() {
    // `poke` closes the previous section with a `---` rule.
    let log = "# T\n\n## Next steps\n\n---\n\n## Investigation\n\nBody.\n\n---\n\n## 2026-09-21 12:19\n\nMore.\n";
    assert_eq!(
        chunks("T", log),
        vec![
            chunk(7, "Investigation", "# T\n## Investigation\n\nBody."),
            chunk(13, "2026-09-21 12:19", "# T\n## 2026-09-21 12:19\n\nMore."),
        ]
    );
}

#[test]
fn subsections_stay_in_their_section() {
    let log = "# T\n\n## Results\n\nFirst.\n\n### Detail\n\nSecond.\n";
    assert_eq!(
        chunks("T", log),
        vec![chunk(
            3,
            "Results",
            "# T\n## Results\n\nFirst.\n\n### Detail\n\nSecond."
        )]
    );
}

#[test]
fn headings_inside_code_blocks_are_not_sections() {
    let log = "# T\n\n## Script\n\n```sh\n## not a heading\necho hi\n```\n";
    assert_eq!(
        chunks("T", log),
        vec![chunk(
            3,
            "Script",
            "# T\n## Script\n\n```sh\n## not a heading\necho hi\n```"
        )]
    );
}

fn words(n: usize, word: &str) -> String {
    vec![word; n].join(" ")
}

#[test]
fn long_sections_are_split_at_paragraphs() {
    let half = MAX_WORDS / 2 + 1;
    let (a, b, c) = (words(half, "a"), words(half, "b"), words(half, "c"));
    let log = format!("# T\n\n## Long\n\n{a}\n\n{b}\n\n{c}\n");
    assert_eq!(
        chunks("T", &log),
        vec![
            chunk(3, "Long", &format!("# T\n## Long\n\n{a}")),
            chunk(7, "Long", &format!("# T\n## Long\n\n{b}")),
            chunk(9, "Long", &format!("# T\n## Long\n\n{c}")),
        ]
    );
}

#[test]
fn split_pieces_pack_as_many_paragraphs_as_fit() {
    let third = MAX_WORDS / 3;
    let (a, b, c) = (words(third, "a"), words(third, "b"), words(third, "c"));
    let d = words(MAX_WORDS / 2, "d");
    let log = format!("# T\n\n## Long\n\n{a}\n\n{b}\n\n{c}\n\n{d}\n");
    assert_eq!(
        chunks("T", &log),
        vec![
            chunk(3, "Long", &format!("# T\n## Long\n\n{a}\n\n{b}\n\n{c}")),
            chunk(11, "Long", &format!("# T\n## Long\n\n{d}")),
        ]
    );
}

#[test]
fn a_paragraph_longer_than_the_limit_is_kept_whole() {
    let big = words(MAX_WORDS + 10, "x");
    let log = format!("# T\n\n## Long\n\n{big}\n");
    assert_eq!(
        chunks("T", &log),
        vec![chunk(3, "Long", &format!("# T\n## Long\n\n{big}"))]
    );
}

// ---------------------------------------------------------------------------
// refresh
// ---------------------------------------------------------------------------

const MODEL: &str = "model-a";

/// A fake embedder that records every text it is asked to embed and returns
/// `[len, 1.0]` for each.
struct Fake {
    seen: RefCell<Vec<String>>,
}

impl Fake {
    fn new() -> Self {
        Self {
            seen: RefCell::new(Vec::new()),
        }
    }

    fn embed(&self) -> impl FnOnce(&[&str]) -> Result<Vec<Vec<f32>>> + '_ {
        |texts: &[&str]| {
            self.seen
                .borrow_mut()
                .extend(texts.iter().map(|t| t.to_string()));
            Ok(texts.iter().map(|t| vec![t.len() as f32, 1.0]).collect())
        }
    }

    fn seen(&self) -> Vec<String> {
        self.seen.borrow().clone()
    }
}

fn entry(log: u32, c: Chunk, vector: Vec<f32>) -> Entry {
    Entry {
        log,
        chunk: c,
        vector,
    }
}

fn empty_index() -> Index {
    Index {
        model: MODEL.to_string(),
        entries: vec![],
    }
}

#[test]
fn a_new_index_embeds_every_chunk() {
    let fake = Fake::new();
    let new = vec![(1, chunk(6, "A", "alpha")), (2, chunk(6, "B", "beta"))];
    let index = refresh(empty_index(), MODEL, new, fake.embed()).unwrap();
    assert_eq!(fake.seen(), vec!["alpha", "beta"]);
    assert_eq!(
        index,
        Index {
            model: MODEL.to_string(),
            entries: vec![
                entry(1, chunk(6, "A", "alpha"), vec![5.0, 1.0]),
                entry(2, chunk(6, "B", "beta"), vec![4.0, 1.0]),
            ],
        }
    );
}

#[test]
fn unchanged_chunks_are_not_embedded_again() {
    let old = Index {
        model: MODEL.to_string(),
        entries: vec![entry(1, chunk(6, "A", "alpha"), vec![9.0, 9.0])],
    };
    let fake = Fake::new();
    let index = refresh(
        old.clone(),
        MODEL,
        vec![(1, chunk(6, "A", "alpha"))],
        fake.embed(),
    )
    .unwrap();
    assert!(fake.seen().is_empty(), "{:?}", fake.seen());
    assert_eq!(index, old);
}

#[test]
fn only_new_or_changed_chunks_are_embedded() {
    let old = Index {
        model: MODEL.to_string(),
        entries: vec![
            entry(1, chunk(6, "A", "alpha"), vec![9.0, 9.0]),
            entry(1, chunk(10, "B", "beta"), vec![8.0, 8.0]),
        ],
    };
    let new = vec![
        (1, chunk(6, "A", "alpha")),
        (1, chunk(10, "B", "beta, fixed")),
        (1, chunk(14, "C", "gamma")),
    ];
    let fake = Fake::new();
    let index = refresh(old, MODEL, new, fake.embed()).unwrap();
    assert_eq!(fake.seen(), vec!["beta, fixed", "gamma"]);
    assert_eq!(
        index.entries,
        vec![
            entry(1, chunk(6, "A", "alpha"), vec![9.0, 9.0]),
            entry(1, chunk(10, "B", "beta, fixed"), vec![11.0, 1.0]),
            entry(1, chunk(14, "C", "gamma"), vec![5.0, 1.0]),
        ]
    );
}

#[test]
fn a_moved_chunk_keeps_its_vector_and_takes_its_new_line() {
    let old = Index {
        model: MODEL.to_string(),
        entries: vec![entry(1, chunk(6, "A", "alpha"), vec![9.0, 9.0])],
    };
    let fake = Fake::new();
    let index = refresh(old, MODEL, vec![(1, chunk(8, "A", "alpha"))], fake.embed()).unwrap();
    assert!(fake.seen().is_empty());
    assert_eq!(
        index.entries,
        vec![entry(1, chunk(8, "A", "alpha"), vec![9.0, 9.0])]
    );
}

#[test]
fn chunks_of_deleted_logs_are_dropped() {
    let old = Index {
        model: MODEL.to_string(),
        entries: vec![
            entry(1, chunk(6, "A", "alpha"), vec![9.0, 9.0]),
            entry(2, chunk(6, "B", "beta"), vec![8.0, 8.0]),
        ],
    };
    let fake = Fake::new();
    let index = refresh(old, MODEL, vec![(1, chunk(6, "A", "alpha"))], fake.embed()).unwrap();
    assert_eq!(
        index.entries,
        vec![entry(1, chunk(6, "A", "alpha"), vec![9.0, 9.0])]
    );
}

#[test]
fn the_same_text_in_another_log_is_its_own_chunk() {
    let old = Index {
        model: MODEL.to_string(),
        entries: vec![entry(1, chunk(6, "A", "alpha"), vec![9.0, 9.0])],
    };
    let fake = Fake::new();
    let index = refresh(old, MODEL, vec![(2, chunk(6, "A", "alpha"))], fake.embed()).unwrap();
    assert_eq!(fake.seen(), vec!["alpha"]);
    assert_eq!(
        index.entries,
        vec![entry(2, chunk(6, "A", "alpha"), vec![5.0, 1.0])]
    );
}

#[test]
fn another_model_re_embeds_everything() {
    let old = Index {
        model: "model-b".to_string(),
        entries: vec![entry(1, chunk(6, "A", "alpha"), vec![9.0, 9.0])],
    };
    let fake = Fake::new();
    let index = refresh(old, MODEL, vec![(1, chunk(6, "A", "alpha"))], fake.embed()).unwrap();
    assert_eq!(fake.seen(), vec!["alpha"]);
    assert_eq!(index.model, MODEL);
    assert_eq!(
        index.entries,
        vec![entry(1, chunk(6, "A", "alpha"), vec![5.0, 1.0])]
    );
}

#[test]
fn embedding_errors_are_passed_on() {
    let failing = |_: &[&str]| -> Result<Vec<Vec<f32>>> { anyhow::bail!("model exploded") };
    let err = refresh(
        empty_index(),
        MODEL,
        vec![(1, chunk(6, "A", "alpha"))],
        failing,
    )
    .unwrap_err();
    assert!(err.to_string().contains("model exploded"), "{err}");
}

// ---------------------------------------------------------------------------
// ranking
// ---------------------------------------------------------------------------

fn vectors(vs: &[[f32; 2]]) -> Vec<Entry> {
    vs.iter()
        .enumerate()
        .map(|(i, v)| entry(i as u32, chunk(1, "H", "t"), v.to_vec()))
        .collect()
}

#[test]
fn ranks_by_cosine_similarity_best_first() {
    let entries = vectors(&[[0.0, 1.0], [1.0, 0.0], [1.0, 1.0]]);
    let hits = top_k(&[1.0, 0.0], &entries, 3);
    let order: Vec<usize> = hits.iter().map(|(i, _)| *i).collect();
    assert_eq!(order, vec![1, 2, 0]);
    assert!((hits[0].1 - 1.0).abs() < 1e-6);
    assert!((hits[1].1 - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-6);
    assert!(hits[2].1.abs() < 1e-6);
}

#[test]
fn ranking_ignores_vector_length() {
    let entries = vectors(&[[10.0, 1.0], [1.0, 0.0]]);
    let hits = top_k(&[1.0, 0.0], &entries, 1);
    assert_eq!(hits[0].0, 1);
}

#[test]
fn returns_at_most_k_hits() {
    let entries = vectors(&[[0.0, 1.0], [1.0, 0.0], [1.0, 1.0]]);
    assert_eq!(top_k(&[1.0, 0.0], &entries, 2).len(), 2);
    assert_eq!(top_k(&[1.0, 0.0], &entries, 10).len(), 3);
    assert!(top_k(&[1.0, 0.0], &[], 5).is_empty());
}

// ---------------------------------------------------------------------------
// index file
// ---------------------------------------------------------------------------

fn sample_index() -> Index {
    Index {
        model: "google/embeddinggemma-300m".to_string(),
        entries: vec![
            entry(
                3,
                chunk(
                    6,
                    "Contexte",
                    "# Précision numérique\n## Contexte\n\nÉlément fini.",
                ),
                vec![0.25, -1.5, 3.0],
            ),
            entry(
                42,
                chunk(120, "Results", "# T\n## Results\n\nk21 → **wrong**"),
                vec![1.0, 0.0, -0.0],
            ),
        ],
    }
}

#[test]
fn the_index_round_trips() {
    let index = sample_index();
    assert_eq!(decode(&encode(&index)).unwrap(), index);
}

#[test]
fn an_empty_index_round_trips() {
    let index = empty_index();
    assert_eq!(decode(&encode(&index)).unwrap(), index);
}

#[test]
fn a_truncated_index_is_an_error() {
    let bytes = encode(&sample_index());
    for len in [0, 1, bytes.len() / 2, bytes.len() - 1] {
        assert!(decode(&bytes[..len]).is_err(), "decoded {len} bytes");
    }
}

#[test]
fn trailing_bytes_are_an_error() {
    let mut bytes = encode(&sample_index());
    bytes.push(0);
    assert!(decode(&bytes).is_err());
}

#[test]
fn a_file_that_is_not_an_index_is_an_error() {
    assert!(decode(b"# Some markdown\n\nNot an index at all.\n").is_err());
}

// ---------------------------------------------------------------------------
// .gitignore
// ---------------------------------------------------------------------------

#[test]
fn gitignore_gains_the_index() {
    assert_eq!(
        with_index_ignored("/target\n").as_deref(),
        Some("/target\nsond/.index\n")
    );
}

#[test]
fn gitignore_without_final_newline_gains_the_index_on_its_own_line() {
    assert_eq!(
        with_index_ignored("/target").as_deref(),
        Some("/target\nsond/.index\n")
    );
}

#[test]
fn empty_gitignore_gains_the_index() {
    assert_eq!(with_index_ignored("").as_deref(), Some("sond/.index\n"));
}

#[test]
fn gitignore_already_ignoring_the_index_is_left_alone() {
    for g in [
        "sond/.index\n",
        "/target\nsond/.index\n",
        "/sond/.index\n",
        "sond/.index   \n",
        "sond/.index",
    ] {
        assert_eq!(with_index_ignored(g), None, "{g:?}");
    }
}

// ---------------------------------------------------------------------------
// model cache location
// ---------------------------------------------------------------------------

fn os(s: &str) -> Option<OsString> {
    Some(OsString::from(s))
}

#[test]
fn model_cache_honours_xdg_cache_home() {
    assert_eq!(
        model_cache_dir_from(os("/xdg"), os("/home/me")),
        Some(PathBuf::from("/xdg/sond"))
    );
}

#[test]
fn model_cache_falls_back_to_dot_cache() {
    assert_eq!(
        model_cache_dir_from(None, os("/home/me")),
        Some(PathBuf::from("/home/me/.cache/sond"))
    );
}

#[test]
fn empty_or_relative_xdg_cache_home_is_ignored() {
    let expected = Some(PathBuf::from("/home/me/.cache/sond"));
    assert_eq!(model_cache_dir_from(os(""), os("/home/me")), expected);
    assert_eq!(
        model_cache_dir_from(os("rel/dir"), os("/home/me")),
        expected
    );
}

#[test]
fn no_cache_location_means_no_model_cache() {
    assert_eq!(model_cache_dir_from(None, None), None);
    assert_eq!(model_cache_dir_from(os(""), os("")), None);
}
