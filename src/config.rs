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
///
/// All other top-level keys are variables: `radius: 25` replaces `{{ radius }}` in each URL.
#[derive(Debug, Deserialize)]
struct SourcesFile {
    #[serde(default)]
    sources: Vec<Source>,
    #[serde(flatten)]
    vars: BTreeMap<String, serde_yaml::Value>,
}

impl SourcesFile {
    /// Replaces each `{{ name }}` in the URLs with its variable. An unknown name is an error.
    fn expand(mut self) -> Res<Vec<Source>> {
        for source in &mut self.sources {
            source.url = expand_vars(&source.url, &self.vars)
                .map_err(|e| format!("source '{}': {e}", source.name))?;
        }
        Ok(self.sources)
    }
}

fn expand_vars(text: &str, vars: &BTreeMap<String, serde_yaml::Value>) -> Res<String> {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("{{") {
        let end = rest[start..]
            .find("}}")
            .ok_or_else(|| format!("'{{{{' without '}}}}' in {text}"))?
            + start;
        let name = rest[start + 2..end].trim();
        let value = match vars.get(name) {
            Some(serde_yaml::Value::String(s)) => s.clone(),
            Some(serde_yaml::Value::Number(n)) => n.to_string(),
            Some(serde_yaml::Value::Bool(b)) => b.to_string(),
            Some(_) => return Err(format!("variable '{name}' must be a text or a number").into()),
            None => {
                let known: Vec<_> = vars.keys().map(String::as_str).collect();
                return Err(format!(
                    "unknown variable '{name}' (defined: {})",
                    if known.is_empty() {
                        "none".to_string()
                    } else {
                        known.join(", ")
                    }
                )
                .into());
            }
        };
        out.push_str(&rest[..start]);
        out.push_str(&value);
        rest = &rest[end + 2..];
    }
    out.push_str(rest);
    Ok(out)
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
    file.expand()
        .map_err(|e| format!("invalid {path}: {e}").into())
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
    /// Sampling temperature. `0` gives the same reply for the same input (useful for demos and
    /// tests). Without it, the provider's default is used.
    pub temperature: Option<f32>,
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
            temperature: None,
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
            let Ok(text) = fs::read_to_string(path) else {
                continue;
            };
            let file: SourcesFile = serde_yaml::from_str(&text).expect(path);
            let sources = file.expand().expect(path);
            assert!(!sources.is_empty(), "{path} has no sources");
            assert!(sources.iter().all(|s| !s.url.contains("{{")), "{path}");
        }
    }

    #[test]
    fn expands_variables() {
        let text = "radius: 25\ncity: witten\nsources:\n  - name: A\n    url: https://a.de/?wo={{city}}&r={{ radius }}\n";
        let file: SourcesFile = serde_yaml::from_str(text).unwrap();
        assert_eq!(
            file.expand().unwrap()[0].url,
            "https://a.de/?wo=witten&r=25"
        );

        let bad: SourcesFile =
            serde_yaml::from_str("sources:\n  - name: A\n    url: https://a.de/?r={{ radius }}\n")
                .unwrap();
        assert!(
            bad.expand()
                .unwrap_err()
                .to_string()
                .contains("unknown variable 'radius'")
        );
    }
}
