use std::{collections::BTreeMap, env, fs, path::Path};

use serde::Deserialize;

use crate::Res;

const DEFAULT_CONFIG_PATH: &str = "config.yaml";

/// Top-level settings, loaded from `config.yaml` (or the file named by `AGENTS_CONFIG`).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Name of the provider entry to use.
    pub active: String,
    /// Named provider entries. Add as many as you like.
    pub providers: BTreeMap<String, ProviderConfig>,
    /// Language for the descriptions and match reasons in the CSV (for example `German`).
    #[serde(default = "default_output_language")]
    pub output_language: String,
}

const DEFAULT_SOURCES_PATH: &str = "sources.yaml";

/// The search pages file, loaded from `sources.yaml` (or the file named by `JOB_RADAR_SOURCES`).
/// It is separate from the config so that it can stay out of Git.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SourcesFile {
    #[serde(default)]
    sources: Vec<Source>,
}

/// Loads the search pages. A missing default file gives an empty list.
pub fn load_sources() -> Res<Vec<Source>> {
    let (path, explicit) = match env::var("JOB_RADAR_SOURCES") {
        Ok(p) if !p.trim().is_empty() => (p, true),
        _ => (DEFAULT_SOURCES_PATH.to_string(), false),
    };
    if !explicit && !Path::new(&path).exists() {
        return Ok(Vec::new());
    }
    let text = fs::read_to_string(&path).map_err(|e| format!("cannot read {path}: {e}"))?;
    let file: SourcesFile =
        serde_yaml::from_str(&text).map_err(|e| format!("invalid {path}: {e}"))?;
    Ok(file.sources)
}

/// One search page to read.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    /// A short label for log output.
    pub name: String,
    pub url: String,
    /// Set to `false` to skip this source without deleting it.
    #[serde(default = "enabled_default")]
    pub enabled: bool,
}

fn enabled_default() -> bool {
    true
}

fn default_output_language() -> String {
    "English".to_string()
}

/// One provider entry. Only `type` and `model` are required; the rest has defaults per type.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderConfig {
    #[serde(rename = "type")]
    pub kind: ProviderKind,
    pub model: String,
    pub base_url: Option<String>,
    /// Name of the environment variable that holds the API token.
    pub api_key_env: Option<String>,
    /// Upper limit for the length of each reply.
    pub max_tokens: Option<u32>,
    /// How much the model thinks before it replies (for example `low`, `medium`, `high`).
    /// Sent as `output_config.effort` (anthropic) or `reasoning_effort` (openai).
    /// For ollama, any value turns thinking on; without it, thinking is off.
    pub effort: Option<String>,
    /// Context window in tokens (prompt + reply). Long pages are read in parts that fit into it.
    /// Ollama also gets it as `num_ctx`: Ollama's own default is small and cuts long prompts
    /// without an error.
    pub context_length: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderKind {
    /// A local model served by Ollama. Needs no token.
    Ollama,
    /// Any API that implements the OpenAI chat completions endpoint.
    OpenAi,
    /// The Anthropic Messages API (Claude models).
    #[serde(alias = "claude")]
    Anthropic,
}

impl ProviderKind {
    pub fn default_base_url(self) -> &'static str {
        match self {
            Self::Ollama => "http://localhost:11434",
            Self::OpenAi => "https://api.openai.com/v1",
            Self::Anthropic => "https://api.anthropic.com/v1",
        }
    }

    /// The context window in tokens if the config does not set one.
    pub fn default_context_length(self) -> u32 {
        match self {
            Self::Ollama => 32_768,
            Self::OpenAi => 128_000,
            Self::Anthropic => 200_000,
        }
    }

    pub fn default_api_key_env(self) -> Option<&'static str> {
        match self {
            Self::Ollama => None,
            Self::OpenAi => Some("OPENAI_API_KEY"),
            Self::Anthropic => Some("ANTHROPIC_API_KEY"),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        let local = ProviderConfig {
            kind: ProviderKind::Ollama,
            model: "qwen3:1.7b".to_string(),
            base_url: None,
            api_key_env: None,
            max_tokens: None,
            effort: None,
            context_length: None,
        };
        Self {
            active: "local".to_string(),
            providers: BTreeMap::from([("local".to_string(), local)]),
            output_language: default_output_language(),
        }
    }
}

impl Config {
    /// Loads the config file. If the default file does not exist, built-in defaults apply.
    pub fn load() -> Res<Self> {
        let (path, explicit) = match env::var("AGENTS_CONFIG") {
            Ok(p) if !p.trim().is_empty() => (p, true),
            _ => (DEFAULT_CONFIG_PATH.to_string(), false),
        };

        if !explicit && !Path::new(&path).exists() {
            return Ok(Self::default());
        }

        let text = fs::read_to_string(&path).map_err(|e| format!("cannot read {path}: {e}"))?;
        let config = serde_yaml::from_str(&text).map_err(|e| format!("invalid {path}: {e}"))?;
        Ok(config)
    }

    /// Returns the name and settings of the active provider.
    pub fn active_provider(&self) -> Res<(&str, &ProviderConfig)> {
        let provider = self.providers.get(&self.active).ok_or_else(|| {
            let known: Vec<_> = self.providers.keys().map(String::as_str).collect();
            format!(
                "unknown provider '{}' (configured: {})",
                self.active,
                known.join(", ")
            )
        })?;
        Ok((&self.active, provider))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sources_files_parse() {
        for path in ["sources.example.yaml", "sources.yaml"] {
            let Ok(text) = fs::read_to_string(path) else { continue };
            let file: SourcesFile = serde_yaml::from_str(&text).expect(path);
            assert!(!file.sources.is_empty(), "{path} has no sources");
        }
    }
}
