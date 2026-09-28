//! The semantic index `sond ask` searches: one vector per chunk of every log,
//! kept in `sond/.index` and refreshed incrementally.

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::PathBuf;

use anyhow::{Result, bail, ensure};

use crate::chunk::Chunk;

/// The index file, inside the logs directory.
pub const INDEX_FILE: &str = ".index";

/// The `.gitignore` line that keeps the index out of Git.
const IGNORE_LINE: &str = "sond/.index";

/// First bytes of an index file, with the format version.
const MAGIC: &[u8] = b"sond-index-1\n";

#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    /// ID of the log the chunk comes from.
    pub log: u32,
    pub chunk: Chunk,
    pub vector: Vec<f32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Index {
    /// The embedding model that produced the vectors.
    pub model: String,
    pub entries: Vec<Entry>,
}

/// The index for `chunks`, as embedded by `model`. Vectors of `old` are
/// reused for chunks with the same log and text; `embed` is called once, with
/// the texts of all the others, and not at all when there are none. An `old`
/// index from another model is ignored.
pub fn refresh(
    old: Index,
    model: &str,
    chunks: Vec<(u32, Chunk)>,
    embed: impl FnOnce(&[&str]) -> Result<Vec<Vec<f32>>>,
) -> Result<Index> {
    let mut known: HashMap<(u32, String), Vec<f32>> = HashMap::new();
    if old.model == model {
        for e in old.entries {
            known.insert((e.log, e.chunk.text), e.vector);
        }
    }
    let reused: Vec<Option<Vec<f32>>> = chunks
        .iter()
        .map(|(log, c)| known.remove(&(*log, c.text.clone())))
        .collect();

    let missing: Vec<&str> = chunks
        .iter()
        .zip(&reused)
        .filter(|(_, v)| v.is_none())
        .map(|((_, c), _)| c.text.as_str())
        .collect();
    let fresh = if missing.is_empty() {
        Vec::new()
    } else {
        embed(&missing)?
    };
    ensure!(
        fresh.len() == missing.len(),
        "the model returned {} vectors for {} texts",
        fresh.len(),
        missing.len()
    );

    let mut fresh = fresh.into_iter();
    let entries = chunks
        .into_iter()
        .zip(reused)
        .map(|((log, chunk), vector)| Entry {
            log,
            chunk,
            vector: vector.or_else(|| fresh.next()).unwrap(),
        })
        .collect();
    Ok(Index {
        model: model.to_string(),
        entries,
    })
}

/// The `k` entries closest to `query` by cosine similarity, best first, as
/// (position in `entries`, similarity).
pub fn top_k(query: &[f32], entries: &[Entry], k: usize) -> Vec<(usize, f32)> {
    let mut hits: Vec<(usize, f32)> = entries
        .iter()
        .enumerate()
        .map(|(i, e)| (i, cosine(query, &e.vector)))
        .collect();
    hits.sort_by(|a, b| b.1.total_cmp(&a.1));
    hits.truncate(k);
    hits
}

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let norms =
        a.iter().map(|x| x * x).sum::<f32>().sqrt() * b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norms == 0.0 { 0.0 } else { dot / norms }
}

/// The index as bytes: [`MAGIC`], the model, the entry count, then each
/// entry. Integers are little-endian `u32`, strings are length-prefixed
/// UTF-8, vectors are length-prefixed little-endian `f32`.
pub fn encode(index: &Index) -> Vec<u8> {
    let mut out = MAGIC.to_vec();
    put_str(&mut out, &index.model);
    put_u32(&mut out, index.entries.len());
    for e in &index.entries {
        put_u32(&mut out, e.log as usize);
        put_u32(&mut out, e.chunk.line);
        put_str(&mut out, &e.chunk.heading);
        put_str(&mut out, &e.chunk.text);
        put_u32(&mut out, e.vector.len());
        for x in &e.vector {
            out.extend_from_slice(&x.to_le_bytes());
        }
    }
    out
}

fn put_u32(out: &mut Vec<u8>, n: usize) {
    out.extend_from_slice(&(n as u32).to_le_bytes());
}

fn put_str(out: &mut Vec<u8>, s: &str) {
    put_u32(out, s.len());
    out.extend_from_slice(s.as_bytes());
}

/// Reads what [`encode`] wrote; anything else is an error.
pub fn decode(bytes: &[u8]) -> Result<Index> {
    let mut r = Reader { bytes };
    ensure!(r.take(MAGIC.len())? == MAGIC, "not a sond index");
    let model = r.string()?;
    let count = r.u32()?;
    let mut entries = Vec::new();
    for _ in 0..count {
        let log = r.u32()? as u32;
        let line = r.u32()?;
        let heading = r.string()?;
        let text = r.string()?;
        let dim = r.u32()?;
        let vector = r
            .take(dim.saturating_mul(4))?
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| f32::from_le_bytes(*b))
            .collect();
        entries.push(Entry {
            log,
            chunk: Chunk {
                line,
                heading,
                text,
            },
            vector,
        });
    }
    ensure!(
        r.bytes.is_empty(),
        "unexpected data at the end of the index"
    );
    Ok(Index { model, entries })
}

struct Reader<'a> {
    bytes: &'a [u8],
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        if n > self.bytes.len() {
            bail!("the index is truncated");
        }
        let (head, rest) = self.bytes.split_at(n);
        self.bytes = rest;
        Ok(head)
    }

    fn u32(&mut self) -> Result<usize> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()) as usize)
    }

    fn string(&mut self) -> Result<String> {
        let n = self.u32()?;
        Ok(String::from_utf8(self.take(n)?.to_vec())?)
    }
}

/// `gitignore` with the index added on a line of its own, or `None` when it
/// already ignores it.
pub fn with_index_ignored(gitignore: &str) -> Option<String> {
    if gitignore
        .lines()
        .any(|l| l.trim().trim_start_matches('/') == IGNORE_LINE)
    {
        return None;
    }
    let mut out = gitignore.to_string();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(IGNORE_LINE);
    out.push('\n');
    Some(out)
}

/// Where the embedding model is kept, shared by every project:
/// `$XDG_CACHE_HOME/sond`, else `~/.cache/sond`. Per the XDG spec, an empty
/// or relative `XDG_CACHE_HOME` is ignored.
pub fn model_cache_dir_from(
    xdg_cache_home: Option<OsString>,
    home: Option<OsString>,
) -> Option<PathBuf> {
    let cache = xdg_cache_home
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| {
            home.filter(|h| !h.is_empty())
                .map(|h| PathBuf::from(h).join(".cache"))
        })?;
    Some(cache.join("sond"))
}
