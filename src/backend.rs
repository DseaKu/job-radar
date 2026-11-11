use std::env;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};

use crate::Res;
use crate::config::{Config, ProviderKind};

const ANTHROPIC_VERSION: &str = "2023-06-01";
const DEFAULT_MAX_TOKENS: u32 = 4096;
/// Rough size of one token in characters. Set low on purpose, so that the estimate is safe
/// for German text and long URLs.
const CHARS_PER_TOKEN: usize = 3;

/// The reply hit `max_tokens` and is not complete. `partial` holds the text up to the cut.
#[derive(Debug)]
pub struct ReplyCutOff {
    pub max_tokens: u32,
    pub partial: String,
}

impl std::fmt::Display for ReplyCutOff {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "reply cut off at max_tokens ({}); increase max_tokens in config.yaml",
            self.max_tokens
        )
    }
}

impl std::error::Error for ReplyCutOff {}

/// The LLM provider that the agents talk to, built from the active config entry.
///
/// Ollama uses its native chat API (the only one that accepts a context size),
/// OpenAI-compatible APIs use chat completions, and Anthropic uses the Messages API.
pub struct Backend {
    name: String,
    kind: ProviderKind,
    http: reqwest::Client,
    base_url: String,
    api_key: Option<String>,
    model: String,
    max_tokens: u32,
    effort: Option<String>,
    context_length: u32,
    /// Token totals over all calls of this run.
    total_in: AtomicU64,
    total_out: AtomicU64,
    calls: AtomicU64,
}

impl Backend {
    /// Builds the backend for the active provider in the config.
    pub fn from_config(config: &Config) -> Res<Self> {
        let (name, p) = config.active_provider()?;

        let api_key = match p.api_key_env.as_deref().or(p.kind.default_api_key_env()) {
            Some(var) => Some(env::var(var).map_err(|_| {
                format!("provider '{name}' requires the {var} environment variable")
            })?),
            None => None,
        };

        let mut base_url = p
            .base_url
            .as_deref()
            .unwrap_or(p.kind.default_base_url())
            .trim_end_matches('/')
            .to_string();
        if p.kind == ProviderKind::Ollama {
            // The native API lives at the server root; accept old `.../v1` addresses too.
            if let Some(root) = base_url.strip_suffix("/v1") {
                base_url = root.to_string();
            }
        }

        Ok(Self {
            name: name.to_string(),
            kind: p.kind,
            http: reqwest::Client::new(),
            base_url,
            api_key,
            model: p.model.clone(),
            max_tokens: p.max_tokens.unwrap_or(DEFAULT_MAX_TOKENS),
            effort: p.effort.clone(),
            context_length: p.context_length.unwrap_or(p.kind.default_context_length()),
            total_in: AtomicU64::new(0),
            total_out: AtomicU64::new(0),
            calls: AtomicU64::new(0),
        })
    }

    /// Sends one system + user message pair and returns the assistant's reply.
    pub async fn chat(&self, system: &str, user: &str) -> Res<String> {
        match self.kind {
            ProviderKind::Ollama => self.chat_ollama(system, user).await,
            ProviderKind::OpenAi => self.chat_openai(system, user).await,
            ProviderKind::Anthropic => self.chat_anthropic(system, user).await,
        }
    }

    async fn chat_ollama(&self, system: &str, user: &str) -> Res<String> {
        let body = json!({
            "model": self.model,
            "stream": false,
            // Thinking only when an effort is configured: it costs time and reply tokens.
            "think": self.effort.is_some(),
            "messages": [
                { "role": "system", "content": system },
                { "role": "user", "content": user },
            ],
            "options": {
                "num_ctx": self.context_length,
                "num_predict": self.max_tokens,
            },
        });
        let request = self
            .http
            .post(format!("{}/api/chat", self.base_url))
            .json(&body);
        let payload = send(request).await?;
        self.record_usage(&payload["prompt_eval_count"], &payload["eval_count"]);

        // Ollama cuts a prompt that is too long without an error. Detect it from the token count.
        let prompt_tokens = payload["prompt_eval_count"].as_u64().unwrap_or(0);
        if prompt_tokens + u64::from(self.max_tokens) >= u64::from(self.context_length) {
            eprintln!(
                "  warning: the prompt used {prompt_tokens} of {} context tokens and was possibly cut; \
                 increase context_length in config.yaml",
                self.context_length
            );
        }
        let content = payload["message"]["content"]
            .as_str()
            .ok_or_else(|| format!("unexpected API response: {payload}"))?;
        if payload["done_reason"] == "length" {
            return Err(self.cut_off(content));
        }
        Ok(strip_think(content))
    }

    async fn chat_openai(&self, system: &str, user: &str) -> Res<String> {
        let mut body = json!({
            "model": self.model,
            "max_tokens": self.max_tokens,
            "messages": [
                { "role": "system", "content": system },
                { "role": "user", "content": user },
            ],
        });
        if let Some(effort) = &self.effort {
            body["reasoning_effort"] = json!(effort);
        }
        let mut request = self
            .http
            .post(format!("{}/chat/completions", self.base_url))
            .json(&body);
        if let Some(key) = &self.api_key {
            request = request.bearer_auth(key);
        }
        let payload = send(request).await?;
        self.record_usage(
            &payload["usage"]["prompt_tokens"],
            &payload["usage"]["completion_tokens"],
        );

        let content = payload["choices"][0]["message"]["content"].as_str();
        if payload["choices"][0]["finish_reason"] == "length" {
            return Err(self.cut_off(content.unwrap_or_default()));
        }
        content
            .map(str::to_string)
            .ok_or_else(|| format!("unexpected API response: {payload}").into())
    }

    async fn chat_anthropic(&self, system: &str, user: &str) -> Res<String> {
        let mut body = json!({
            "model": self.model,
            "max_tokens": self.max_tokens,
            "system": system,
            "messages": [{ "role": "user", "content": user }],
        });
        if let Some(effort) = &self.effort {
            body["output_config"] = json!({ "effort": effort });
        }
        let mut request = self
            .http
            .post(format!("{}/messages", self.base_url))
            .header("anthropic-version", ANTHROPIC_VERSION)
            .json(&body);
        if let Some(key) = &self.api_key {
            request = request.header("x-api-key", key);
        }
        let payload = send(request).await?;
        // Input = uncached + cache-read + cache-write tokens.
        let usage = &payload["usage"];
        let input = [
            "input_tokens",
            "cache_read_input_tokens",
            "cache_creation_input_tokens",
        ]
        .iter()
        .filter_map(|k| usage[*k].as_u64())
        .sum::<u64>();
        self.record_usage(&json!(input), &usage["output_tokens"]);

        if payload["stop_reason"] == "refusal" {
            return Err(format!("the model refused: {payload}").into());
        }

        // The reply is a list of content blocks; join the text blocks.
        let blocks = payload["content"]
            .as_array()
            .ok_or_else(|| format!("unexpected API response: {payload}"))?;
        let text = blocks
            .iter()
            .filter(|b| b["type"] == "text")
            .filter_map(|b| b["text"].as_str())
            .collect::<Vec<_>>()
            .join("");
        if payload["stop_reason"] == "max_tokens" {
            return Err(self.cut_off(&text));
        }
        Ok(text)
    }

    fn cut_off(&self, partial: &str) -> Box<dyn std::error::Error> {
        Box::new(ReplyCutOff {
            max_tokens: self.max_tokens,
            partial: strip_think(partial),
        })
    }

    /// Prints the token usage of one call and adds it to the run totals.
    fn record_usage(&self, input: &Value, output: &Value) {
        let (input, output) = (input.as_u64().unwrap_or(0), output.as_u64().unwrap_or(0));
        self.total_in.fetch_add(input, Ordering::Relaxed);
        self.total_out.fetch_add(output, Ordering::Relaxed);
        self.calls.fetch_add(1, Ordering::Relaxed);
        eprintln!("  usage: {input} input tokens, {output} output tokens");
    }

    /// How many characters of page text fit into the context window, after the reply
    /// (`max_tokens`), the other prompt text (`other_chars`) and a 10% safety margin.
    pub fn page_char_budget(&self, other_chars: usize) -> usize {
        let window = self.context_length as usize * 9 / 10;
        let used = self.max_tokens as usize + other_chars.div_ceil(CHARS_PER_TOKEN);
        window.saturating_sub(used) * CHARS_PER_TOKEN
    }

    /// A one-line summary of the token usage over all calls of this run.
    pub fn usage_summary(&self) -> String {
        format!(
            "Total usage: {} input tokens, {} output tokens ({} AI calls)",
            self.total_in.load(Ordering::Relaxed),
            self.total_out.load(Ordering::Relaxed),
            self.calls.load(Ordering::Relaxed),
        )
    }

    /// A short label for the active provider and model, for logging.
    pub fn describe(&self) -> String {
        format!("{} ({} @ {})", self.name, self.model, self.base_url)
    }
}

/// Removes a `<think>...</think>` block that some models put into the reply text.
fn strip_think(text: &str) -> String {
    match (text.find("<think>"), text.find("</think>")) {
        (Some(start), Some(end)) if start < end => {
            format!("{}{}", &text[..start], &text[end + "</think>".len()..])
                .trim()
                .to_string()
        }
        _ => text.to_string(),
    }
}

/// Sends the request and returns the JSON body, or an error with the API's message.
async fn send(request: reqwest::RequestBuilder) -> Res<Value> {
    let response = request.send().await?;
    let status = response.status();
    let payload: Value = response.json().await?;
    if !status.is_success() {
        return Err(format!("API request failed ({status}): {payload}").into());
    }
    Ok(payload)
}
