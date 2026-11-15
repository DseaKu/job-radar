use std::{collections::BTreeMap, env, fs, path::Path};

use serde::{Deserialize, Serialize};

use crate::Res;

const DEFAULT_CONFIG_PATH: &str = "config.yaml";

/// Top-level settings, loaded from `config.yaml` (or the file named by `AGENTS_CONFIG`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Name of the provider entry to use.
    pub active: String,
    /// Named provider entries. Add as many as you like.
    pub providers: BTreeMap<String, ProviderConfig>,
    /// Language for the descriptions and match reasons in the CSV (for example `German`).
    #[serde(default = "default_output_language")]
    pub output_language: String,
    /// Extra instructions from the user for finding and rating the jobs
    /// (for example "Rate remote jobs higher"). Empty: no extra instructions.
    #[serde(default)]
    pub extra_instructions: String,
}

const DEFAULT_SOURCES_PATH: &str = "sources.yaml";

/// The search pages file, loaded from `sources.yaml` (or the file named by `JOB_RADAR_SOURCES`).
/// It is separate from the config so that it can stay out of Git.
///
/// All other top-level keys are variables: `radius: 25` replaces `{{ radius }}` in each URL.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct SourcesFile {
    /// The variables come first, so that they are at the top of a saved file.
    #[serde(flatten)]
    pub vars: BTreeMap<String, serde_yaml::Value>,
    #[serde(default)]
    pub sources: Vec<Source>,
}

impl SourcesFile {
    /// Replaces each `{{ name }}` in the URLs with its variable. An unknown name is an error.
    pub fn expand(mut self) -> Res<Vec<Source>> {
        for source in &mut self.sources {
            source.url = expand_vars(&source.url, &self.vars)
                .map_err(|e| format!("source '{}': {e}", source.name))?;
        }
        Ok(self.sources)
    }
}

/// Replaces each `{{ name }}` in `text` with its variable. An unknown name is an error.
pub fn expand_vars(text: &str, vars: &BTreeMap<String, serde_yaml::Value>) -> Res<String> {
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

/// The path of a settings file: the environment variable `var` if it is set, else `default`.
/// The flag is true for a path from the environment variable.
fn settings_path(var: &str, default: &str) -> (String, bool) {
    match env::var(var) {
        Ok(p) if !p.trim().is_empty() => (p, true),
        _ => (default.to_string(), false),
    }
}

/// The path of the sources file.
pub fn sources_path() -> String {
    settings_path("JOB_RADAR_SOURCES", DEFAULT_SOURCES_PATH).0
}

/// Loads the sources file as written, without the variables replaced.
/// A missing default file gives an empty file.
pub fn load_sources_file() -> Res<SourcesFile> {
    let (path, explicit) = settings_path("JOB_RADAR_SOURCES", DEFAULT_SOURCES_PATH);
    if !explicit && !Path::new(&path).exists() {
        return Ok(SourcesFile::default());
    }
    let text = fs::read_to_string(&path).map_err(|e| format!("cannot read {path}: {e}"))?;
    serde_yaml::from_str(&text).map_err(|e| format!("invalid {path}: {e}").into())
}

/// Loads the search pages, with the variables replaced. A missing default file gives an empty list.
pub fn load_sources() -> Res<Vec<Source>> {
    load_sources_file()?
        .expand()
        .map_err(|e| format!("invalid {}: {e}", sources_path()).into())
}

/// Writes the sources file. The comments of the old file are lost.
pub fn save_sources_file(file: &SourcesFile) -> Res<()> {
    let header = "# Search pages. Written by the Job Radar app; comments are not kept.\n\
                  # Each top-level key except `sources` is a variable: `{{ name }}` in a URL is replaced with its value.\n";
    write_yaml(&sources_path(), header, file)
}

/// Writes `value` as YAML with a comment header.
pub(crate) fn write_yaml(path: &str, header: &str, value: &impl Serialize) -> Res<()> {
    let text = format!("{header}{}", serde_yaml::to_string(value)?);
    fs::write(path, text).map_err(|e| format!("cannot write {path}: {e}").into())
}

/// One search page to read.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderConfig {
    #[serde(rename = "type")]
    pub kind: ProviderKind,
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    /// Name of the environment variable that holds the API token.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key_env: Option<String>,
    /// Upper limit for the length of each reply.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    /// How much the model thinks before it replies (for example `low`, `medium`, `high`).
    /// Sent as `output_config.effort` (anthropic) or `reasoning_effort` (openai).
    /// For ollama, any value turns thinking on; without it, thinking is off.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    /// Context window in tokens (prompt + reply). Long pages are read in parts that fit into it.
    /// Ollama also gets it as `num_ctx`: Ollama's own default is small and cuts long prompts
    /// without an error.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_length: Option<u32>,
    /// Sampling temperature. `0` gives the same reply for the same input (useful for demos and
    /// tests). Without it, the provider's default is used.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
            extra_instructions: String::new(),
        }
    }
}

impl Config {
    /// Loads the config file. If the default file does not exist, built-in defaults apply.
    pub fn load() -> Res<Self> {
        let (path, explicit) = settings_path("AGENTS_CONFIG", DEFAULT_CONFIG_PATH);

        if !explicit && !Path::new(&path).exists() {
            return Ok(Self::default());
        }

        let text = fs::read_to_string(&path).map_err(|e| format!("cannot read {path}: {e}"))?;
        let config = serde_yaml::from_str(&text).map_err(|e| format!("invalid {path}: {e}"))?;
        Ok(config)
    }

    /// The path of the config file.
    pub fn path() -> String {
        settings_path("AGENTS_CONFIG", DEFAULT_CONFIG_PATH).0
    }

    /// Writes the config file. The comments of the old file are lost.
    pub fn save(&self) -> Res<()> {
        self.active_provider()?;
        write_yaml(
            &Self::path(),
            "# Agent settings. Written by the Job Radar app; comments are not kept.\n\
             # All fields are explained in config.example.yaml.\n",
            self,
        )
    }

    /// The environment variable that holds the API key of a provider, or `None` if it needs none.
    pub fn api_key_var(provider: &ProviderConfig) -> Option<&str> {
        provider
            .api_key_env
            .as_deref()
            .or(provider.kind.default_api_key_env())
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
    fn config_files_parse() {
        for path in ["config.example.yaml", "demo/config.yaml"] {
            let text = fs::read_to_string(path).expect(path);
            let config: Config = serde_yaml::from_str(&text).expect(path);
            config.active_provider().expect(path);
        }
    }

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
    fn saved_config_and_sources_parse_again() {
        let text = fs::read_to_string("config.example.yaml").unwrap();
        let config: Config = serde_yaml::from_str(&text).unwrap();
        let again: Config = serde_yaml::from_str(&serde_yaml::to_string(&config).unwrap()).unwrap();
        assert_eq!(again.active, config.active);
        assert_eq!(again.providers.len(), config.providers.len());
        let claude = &again.providers["claude"];
        assert_eq!(claude.kind, ProviderKind::Anthropic);
        assert_eq!(claude.max_tokens, Some(16000));
        assert!(claude.base_url.is_none());

        let text = "radius: 25\nsources:\n  - name: A\n    url: https://a.de/?r={{ radius }}\n    enabled: false\n";
        let file: SourcesFile = serde_yaml::from_str(text).unwrap();
        let saved = serde_yaml::to_string(&file).unwrap();
        assert!(saved.starts_with("radius: 25\n"), "{saved}");
        let again: SourcesFile = serde_yaml::from_str(&saved).unwrap();
        let sources = again.expand().unwrap();
        assert_eq!(sources[0].url, "https://a.de/?r=25");
        assert!(!sources[0].enabled);
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
