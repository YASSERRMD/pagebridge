//! Config file at ~/.pagebridge/config.toml.

#![allow(
    clippy::missing_errors_doc,
    clippy::doc_markdown,
    clippy::option_if_let_else,
    clippy::assigning_clones,
    clippy::derivable_impls,
    clippy::module_name_repetitions
)]

use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PbConfig {
    #[serde(default)]
    pub storage: StorageCfg,
    #[serde(default)]
    pub llm: LlmCfg,
    #[serde(default)]
    pub navigation: NavCfg,
}

impl Default for PbConfig {
    fn default() -> Self {
        Self {
            storage: StorageCfg::default(),
            llm: LlmCfg::default(),
            navigation: NavCfg::default(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StorageCfg {
    #[serde(default = "default_adapter")]
    pub adapter: String,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub database: Option<String>,
}

fn default_adapter() -> String {
    "sqlite".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmCfg {
    #[serde(default = "default_provider")]
    pub provider: String,
    #[serde(default = "default_model")]
    pub model: String,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub api_key: Option<String>,
}

impl Default for LlmCfg {
    fn default() -> Self {
        Self {
            provider: default_provider(),
            model: default_model(),
            base_url: None,
            api_key: None,
        }
    }
}

fn default_provider() -> String {
    "ollama".into()
}
fn default_model() -> String {
    "qwen2.5:7b".into()
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NavCfg {
    pub max_depth: Option<u8>,
    pub beam_width: Option<u8>,
    pub bm25_candidate_limit: Option<usize>,
    pub max_leaves: Option<u8>,
}

impl PbConfig {
    pub fn load(path: &Path) -> Result<Self> {
        let s = std::fs::read_to_string(path)?;
        Ok(toml::from_str(&s)?)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p)?;
        }
        std::fs::write(path, toml::to_string_pretty(self)?)?;
        Ok(())
    }

    pub fn get(&self, key: &str) -> Option<String> {
        match key {
            "storage.adapter" => Some(self.storage.adapter.clone()),
            "storage.path" => self.storage.path.clone(),
            "storage.url" => self.storage.url.clone(),
            "storage.database" => self.storage.database.clone(),
            "llm.provider" => Some(self.llm.provider.clone()),
            "llm.model" => Some(self.llm.model.clone()),
            "llm.base_url" => self.llm.base_url.clone(),
            "llm.api_key" => self.llm.api_key.clone(),
            _ => None,
        }
    }

    pub fn set(&mut self, key: &str, value: &str) -> Result<()> {
        match key {
            "storage.adapter" => self.storage.adapter = value.to_owned(),
            "storage.path" => self.storage.path = Some(value.to_owned()),
            "storage.url" => self.storage.url = Some(value.to_owned()),
            "storage.database" => self.storage.database = Some(value.to_owned()),
            "llm.provider" => self.llm.provider = value.to_owned(),
            "llm.model" => self.llm.model = value.to_owned(),
            "llm.base_url" => self.llm.base_url = Some(value.to_owned()),
            "llm.api_key" => self.llm.api_key = Some(value.to_owned()),
            other => return Err(anyhow!("unknown key: {other}")),
        }
        Ok(())
    }
}

pub fn default_config_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".pagebridge")
        .join("config.toml")
}
