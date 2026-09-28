//! Compares embedding models for `sond ask` on real logs.
//!
//! ```text
//! cargo run --release --features ask --example eval -- <queries.tsv>
//! ```
//!
//! Each line of `queries.tsv` is `<project dir>\t<query>\t<gold>`, where
//! `<gold>` lists the expected sections, space-separated: `R016:30` for the
//! section whose heading is on line 30 of R016, `R022` for any section of
//! R022. Every project is indexed on its own, as `sond index` would, and each
//! query searches its own project only. Models download on first use.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result, bail};
use fastembed::{EmbeddingModel, TextEmbedding};
use sond::chunk::{Chunk, chunks};
use sond::embed::{self, MODEL, Model, Spec};
use sond::index::{Index, refresh, top_k};
use sond::log;

static CANDIDATES: [Spec; 3] = [
    MODEL,
    Spec {
        model: EmbeddingModel::MultilingualE5Small,
        name: "multilingual-e5-small",
        size: "470 MB",
        query_prefix: "query: ",
        passage_prefix: "passage: ",
    },
    Spec {
        model: EmbeddingModel::BGEM3,
        name: "bge-m3",
        size: "2.3 GB",
        query_prefix: "",
        passage_prefix: "",
    },
];

/// A section a query should find: a log, and a heading in it (any, if none).
type Gold = (u32, Option<String>);

struct Query {
    project: PathBuf,
    text: String,
    gold: Vec<Gold>,
}

fn main() -> Result<()> {
    let path = std::env::args()
        .nth(1)
        .context("usage: eval <queries.tsv>")?;
    let tsv = fs::read_to_string(&path).with_context(|| format!("could not read {path}"))?;

    let mut corpora: BTreeMap<PathBuf, Vec<(u32, Chunk)>> = BTreeMap::new();
    let mut queries = Vec::new();
    for (n, line) in tsv
        .lines()
        .enumerate()
        .filter(|(_, l)| !l.trim().is_empty())
    {
        let [project, text, gold] = line.split('\t').collect::<Vec<_>>()[..] else {
            bail!("{path}:{}: expected 3 tab-separated fields", n + 1);
        };
        let project = PathBuf::from(project);
        if !corpora.contains_key(&project) {
            corpora.insert(project.clone(), corpus(&project)?);
        }
        let gold = gold
            .split_whitespace()
            .map(|g| resolve(g, &corpora[&project]))
            .collect::<Result<_>>()
            .with_context(|| format!("{path}:{}", n + 1))?;
        queries.push(Query {
            project,
            text: text.to_string(),
            gold,
        });
    }
    for (project, chunks) in &corpora {
        println!("{}: {} chunks", project.display(), chunks.len());
    }

    let dir = embed::cache_dir().context("no cache directory")?;
    let mut ranks: Vec<Vec<Option<usize>>> = Vec::new();
    println!("\nmodel                      recall@5  MRR@10  index s  cold query s  size MB");
    for spec in &CANDIDATES {
        let mut model = Model::load(spec, &dir, true)?;

        let start = Instant::now();
        let mut indexes = BTreeMap::new();
        for (project, chunks) in &corpora {
            let empty = Index {
                model: String::new(),
                entries: Vec::new(),
            };
            let index = refresh(empty, spec.name, chunks.clone(), |t| model.passages(t))?;
            indexes.insert(project, index);
        }
        let index_secs = start.elapsed().as_secs_f64();

        let mut model_ranks = Vec::new();
        for q in &queries {
            let index = &indexes[&q.project];
            let hits = top_k(&model.query(&q.text)?, &index.entries, 10);
            model_ranks.push(hits.iter().position(|&(i, _)| {
                let e = &index.entries[i];
                q.gold.iter().any(|(log, heading)| {
                    e.log == *log && heading.as_ref().is_none_or(|h| *h == e.chunk.heading)
                })
            }));
        }
        drop(model);

        let start = Instant::now();
        Model::load(spec, &dir, false)?.query("cold start")?;
        let cold_secs = start.elapsed().as_secs_f64();

        let n = queries.len() as f64;
        let recall = model_ranks
            .iter()
            .filter(|r| r.is_some_and(|r| r < 5))
            .count() as f64
            / n;
        let mrr = model_ranks
            .iter()
            .map(|r| r.map_or(0.0, |r| 1.0 / (r + 1) as f64))
            .sum::<f64>()
            / n;
        println!(
            "{:<26} {recall:>8.2}  {mrr:>6.2}  {index_secs:>7.1}  {cold_secs:>12.2}  {:>7.0}",
            spec.name,
            size_mb(spec, &dir)?
        );
        ranks.push(model_ranks);
    }

    println!("\nrank of the first expected section (1-based, - = not in the top 10)");
    for (i, q) in queries.iter().enumerate() {
        let cells: Vec<String> = ranks
            .iter()
            .map(|r| r[i].map_or("-".to_string(), |r| (r + 1).to_string()))
            .collect();
        println!(
            "{:>2}  {:>3} {:>3} {:>3}  {}",
            i + 1,
            cells[0],
            cells[1],
            cells[2],
            q.text
        );
    }
    Ok(())
}

/// Every chunk of the logs in `<project>/sond`.
fn corpus(project: &Path) -> Result<Vec<(u32, Chunk)>> {
    let logs = log::load_logs(&project.join("sond"))?;
    if logs.is_empty() {
        bail!("no logs in {}/sond", project.display());
    }
    Ok(logs
        .iter()
        .flat_map(|(l, content)| chunks(&l.title, content).into_iter().map(|c| (l.id, c)))
        .collect())
}

/// `R016:30` or `R022`, checked against the corpus.
fn resolve(gold: &str, corpus: &[(u32, Chunk)]) -> Result<Gold> {
    let (id, line) = match gold.split_once(':') {
        Some((id, line)) => (id, Some(line.parse::<usize>()?)),
        None => (gold, None),
    };
    let id = log::parse_id(id).with_context(|| format!("bad log ID in {gold:?}"))?;
    let mut sections = corpus.iter().filter(|(log, _)| *log == id);
    match line {
        None if sections.next().is_some() => Ok((id, None)),
        None => bail!("{gold}: no sections in that log"),
        Some(line) => match sections.find(|(_, c)| c.line == line) {
            Some((_, c)) => Ok((id, Some(c.heading.clone()))),
            None => bail!("{gold}: no section starts on that line"),
        },
    }
}

/// Size on disk of the model's weights.
fn size_mb(spec: &Spec, dir: &Path) -> Result<f64> {
    let info = TextEmbedding::get_model_info(&spec.model)?;
    let repo = hf_hub::Cache::new(dir.to_path_buf()).model(info.model_code.clone());
    let mut bytes = 0;
    for file in std::iter::once(&info.model_file).chain(&info.additional_files) {
        let path = repo.get(file).context("model file missing")?;
        bytes += fs::metadata(path)?.len();
    }
    Ok(bytes as f64 / 1e6)
}
