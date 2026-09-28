//! The embedding model behind `sond index` and `sond ask`, run locally.

use std::path::{Path, PathBuf};

use anyhow::Result;
use fastembed::{EmbeddingModel, TextEmbedding, TextInitOptions};

use crate::index::model_cache_dir_from;

/// An embedding model and the prompts it expects in front of each text.
pub struct Spec {
    pub model: EmbeddingModel,
    /// The name recorded in the index; changing it re-embeds everything.
    pub name: &'static str,
    /// Download size, told to the user before downloading.
    pub size: &'static str,
    pub query_prefix: &'static str,
    pub passage_prefix: &'static str,
}

/// The model Sond uses.
pub const MODEL: Spec = Spec {
    model: EmbeddingModel::EmbeddingGemma300MQ4,
    name: "embeddinggemma-300m-q4",
    size: "about 200 MB",
    query_prefix: "task: search result | query: ",
    passage_prefix: "title: none | text: ",
};

/// Where models are downloaded: the per-user cache, unless `HF_HOME` is
/// set, which the download library honours over anything we pass it.
pub fn cache_dir() -> Option<PathBuf> {
    std::env::var_os("HF_HOME")
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            model_cache_dir_from(std::env::var_os("XDG_CACHE_HOME"), std::env::var_os("HOME"))
        })
}

/// Whether the model's weights are already in `dir`, checked without any
/// network access.
pub fn is_installed(spec: &Spec, dir: &Path) -> Result<bool> {
    let info = TextEmbedding::get_model_info(&spec.model)?;
    let repo = hf_hub::Cache::new(dir.to_path_buf()).model(info.model_code.clone());
    Ok(std::iter::once(&info.model_file)
        .chain(&info.additional_files)
        .all(|f| repo.get(f).is_some()))
}

pub struct Model {
    spec: &'static Spec,
    embedding: TextEmbedding,
}

impl Model {
    /// Loads the model from `dir`, downloading whatever is missing.
    pub fn load(spec: &'static Spec, dir: &Path, show_progress: bool) -> Result<Self> {
        let options = TextInitOptions::new(spec.model.clone())
            .with_cache_dir(dir.to_path_buf())
            .with_show_download_progress(show_progress);
        Ok(Self {
            spec,
            embedding: TextEmbedding::try_new(options)?,
        })
    }

    pub fn passages(&mut self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        self.embed(self.spec.passage_prefix, texts)
    }

    pub fn query(&mut self, text: &str) -> Result<Vec<f32>> {
        Ok(self.embed(self.spec.query_prefix, &[text])?.remove(0))
    }

    fn embed(&mut self, prefix: &str, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        let prompts: Vec<String> = texts.iter().map(|t| format!("{prefix}{t}")).collect();
        Ok(self.embedding.embed(prompts, None)?)
    }
}
