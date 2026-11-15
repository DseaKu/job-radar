use std::collections::BTreeMap;
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

/// Token counts. `Backend::usage()` gives the totals of this run; the difference of two
/// snapshots gives the usage of one step (for example one website).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
pub struct Usage {
    pub input: u64,
    pub output: u64,
    pub calls: u64,
}

impl std::ops::Sub for Usage {
    type Output = Usage;
    fn sub(self, before: Usage) -> Usage {
        Usage {
            input: self.input.saturating_sub(before.input),
            output: self.output.saturating_sub(before.output),
            calls: self.calls.saturating_sub(before.calls),
        }
    }
}

impl std::ops::Add for Usage {
    type Output = Usage;
    fn add(self, other: Usage) -> Usage {
        Usage {
            input: self.input + other.input,
            output: self.output + other.output,
            calls: self.calls + other.calls,
        }
    }
}

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
    temperature: Option<f32>,
    /// Extra top-level fields for each request body (`extra_body` in the config).
    extra_body: BTreeMap<String, Value>,
    /// Token totals over all calls of this run.
    total_in: AtomicU64,
    total_out: AtomicU64,
    calls: AtomicU64,
}

impl Backend {
    /// Builds the backend for the active provider in the config.
    /// The API key comes from the environment variable that the provider names.
    pub fn from_config(config: &Config) -> Res<Self> {
        Self::from_config_with_keys(config, |var| env::var(var).ok())
    }

    /// Like [`Backend::from_config`], but `api_key` gives the value of the API key variable
    /// (for example from a `.env` file that was changed after the start).
    pub fn from_config_with_keys(
        config: &Config,
        api_key: impl Fn(&str) -> Option<String>,
    ) -> Res<Self> {
        let (name, p) = config.active_provider()?;

        let api_key =
            match Config::api_key_var(p) {
                Some(var) => Some(api_key(var).filter(|k| !k.trim().is_empty()).ok_or_else(
                    || format!("provider '{name}' requires the {var} environment variable"),
                )?),
                None => None,
            };

        // For Ollama without a base_url in the config, JOB_RADAR_OLLAMA_URL can set the address.
        // The Docker image uses it: inside a container, "localhost" is the container itself.
        let ollama_url = env::var("JOB_RADAR_OLLAMA_URL")
            .ok()
            .filter(|u| p.kind == ProviderKind::Ollama && !u.trim().is_empty());
        let mut base_url = p
            .base_url
            .as_deref()
            .or(ollama_url.as_deref())
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
            temperature: p.temperature,
            extra_body: p.extra_body.clone().unwrap_or_default(),
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
        let request = self
            .http
            .post(format!("{}/api/chat", self.base_url))
            .json(&self.ollama_body(system, user));
        self.read_ollama(&send(request).await?)
    }

    async fn chat_openai(&self, system: &str, user: &str) -> Res<String> {
        let mut request = self
            .http
            .post(format!("{}/chat/completions", self.base_url))
            .json(&self.openai_body(system, user));
        if let Some(key) = &self.api_key {
            request = request.bearer_auth(key);
        }
        self.read_openai(&send(request).await?)
    }

    async fn chat_anthropic(&self, system: &str, user: &str) -> Res<String> {
        let mut request = self
            .http
            .post(format!("{}/messages", self.base_url))
            .header("anthropic-version", ANTHROPIC_VERSION)
            .json(&self.anthropic_body(system, user));
        if let Some(key) = &self.api_key {
            request = request.header("x-api-key", key);
        }
        self.read_anthropic(&send(request).await?)
    }

    fn ollama_body(&self, system: &str, user: &str) -> Value {
        let mut body = json!({
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
        if let Some(t) = self.temperature {
            body["options"]["temperature"] = json!(t);
        }
        merge_extra(&mut body, &self.extra_body);
        body
    }

    fn read_ollama(&self, payload: &Value) -> Res<String> {
        self.record_usage(&payload["prompt_eval_count"], &payload["eval_count"]);

        // Ollama cuts a prompt that is too long without an error. Detect it from the token count.
        let prompt_tokens = payload["prompt_eval_count"].as_u64().unwrap_or(0);
        if prompt_tokens + u64::from(self.max_tokens) >= u64::from(self.context_length) {
            warn!(
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

    fn openai_body(&self, system: &str, user: &str) -> Value {
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
        if let Some(t) = self.temperature {
            body["temperature"] = json!(t);
        }
        merge_extra(&mut body, &self.extra_body);
        body
    }

    fn read_openai(&self, payload: &Value) -> Res<String> {
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

    fn anthropic_body(&self, system: &str, user: &str) -> Value {
        let mut body = json!({
            "model": self.model,
            "max_tokens": self.max_tokens,
            "system": system,
            "messages": [{ "role": "user", "content": user }],
        });
        if let Some(effort) = &self.effort {
            body["output_config"] = json!({ "effort": effort });
        }
        if let Some(t) = self.temperature {
            body["temperature"] = json!(t);
        }
        merge_extra(&mut body, &self.extra_body);
        body
    }

    fn read_anthropic(&self, payload: &Value) -> Res<String> {
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

    fn cut_off(&self, partial: &str) -> Box<dyn std::error::Error + Send + Sync> {
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
        info!("  usage: {input} input tokens, {output} output tokens");
    }

    /// How many characters of page text fit into the context window, after the reply
    /// (`max_tokens`), the other prompt text (`other_chars`) and a 10% safety margin.
    pub fn page_char_budget(&self, other_chars: usize) -> usize {
        let window = self.context_length as usize * 9 / 10;
        let used = self.max_tokens as usize + other_chars.div_ceil(CHARS_PER_TOKEN);
        window.saturating_sub(used) * CHARS_PER_TOKEN
    }

    /// The token totals over all calls of this run so far.
    pub fn usage(&self) -> Usage {
        Usage {
            input: self.total_in.load(Ordering::Relaxed),
            output: self.total_out.load(Ordering::Relaxed),
            calls: self.calls.load(Ordering::Relaxed),
        }
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

/// Copies each extra field into the top level of the request body. A field that the body
/// already has is replaced (the same as `extra_body` in the OpenAI SDK).
fn merge_extra(body: &mut Value, extra: &BTreeMap<String, Value>) {
    if let Some(map) = body.as_object_mut() {
        for (key, value) in extra {
            map.insert(key.clone(), value.clone());
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn config(yaml: &str) -> Config {
        serde_yaml::from_str(yaml).unwrap()
    }

    fn backend(kind: &str, extra: &str) -> Backend {
        let yaml = format!(
            "active: p\nproviders:\n  p:\n    type: {kind}\n    model: m\n    api_key_env: PATH\n    \
             max_tokens: 100\n    context_length: 1000\n{extra}"
        );
        Backend::from_config(&config(&yaml)).unwrap()
    }

    fn cut_off(result: Res<String>) -> ReplyCutOff {
        *result.unwrap_err().downcast::<ReplyCutOff>().unwrap()
    }

    #[test]
    fn extra_body_is_merged_into_each_request() {
        let extra = "    temperature: 0.5\n    extra_body: {\"mode\": \"non-private\", \"temperature\": 0}\n";
        for kind in ["openai", "anthropic", "ollama"] {
            let b = backend(kind, extra);
            let body = match kind {
                "openai" => b.openai_body("sys", "usr"),
                "anthropic" => b.anthropic_body("sys", "usr"),
                _ => b.ollama_body("sys", "usr"),
            };
            assert_eq!(body["mode"], "non-private", "{kind}");
            assert_eq!(body["model"], "m", "{kind}");
        }
        // An existing top-level field is replaced.
        assert_eq!(
            backend("openai", extra).openai_body("s", "u")["temperature"],
            0
        );

        // Without extra_body, the body is the same as before.
        let plain = backend("openai", "    temperature: 0.5\n").openai_body("s", "u");
        let mut merged = plain.clone();
        merge_extra(&mut merged, &BTreeMap::new());
        assert_eq!(merged, plain);
        assert_eq!(
            plain,
            json!({
                "model": "m",
                "max_tokens": 100,
                "messages": [
                    { "role": "system", "content": "s" },
                    { "role": "user", "content": "u" },
                ],
                "temperature": 0.5,
            })
        );
    }

    #[test]
    fn ollama_body_and_reply() {
        let b = backend("ollama", "    temperature: 0\n");
        let body = b.ollama_body("sys", "usr");
        assert_eq!(body["think"], false);
        assert_eq!(body["options"]["num_ctx"], 1000);
        assert_eq!(body["options"]["num_predict"], 100);
        assert_eq!(body["options"]["temperature"], 0.0);
        assert_eq!(body["messages"][1]["content"], "usr");

        let reply = json!({
            "message": { "content": "<think>hmm</think>\n[1]" },
            "prompt_eval_count": 40, "eval_count": 7, "done_reason": "stop"
        });
        assert_eq!(b.read_ollama(&reply).unwrap(), "[1]");
        let usage = Usage {
            input: 40,
            output: 7,
            calls: 1,
        };
        assert_eq!(b.usage(), usage);
    }

    #[test]
    fn ollama_cut_off_reply() {
        let b = backend("ollama", "");
        let reply =
            json!({ "message": { "content": "[{\"title\": \"A\"}, {" }, "done_reason": "length" });
        let cut = cut_off(b.read_ollama(&reply));
        assert_eq!(cut.max_tokens, 100);
        assert_eq!(cut.partial, "[{\"title\": \"A\"}, {");
    }

    #[test]
    fn openai_reply_and_cut_off() {
        let b = backend("openai", "    effort: low\n");
        assert_eq!(b.openai_body("s", "u")["reasoning_effort"], "low");

        let ok = json!({
            "choices": [{ "message": { "content": "[]" }, "finish_reason": "stop" }],
            "usage": { "prompt_tokens": 12, "completion_tokens": 3 }
        });
        assert_eq!(b.read_openai(&ok).unwrap(), "[]");
        let cut =
            json!({ "choices": [{ "message": { "content": "[{" }, "finish_reason": "length" }] });
        assert_eq!(cut_off(b.read_openai(&cut)).partial, "[{");
        assert!(b.read_openai(&json!({ "error": "x" })).is_err());
        let usage = Usage {
            input: 12,
            output: 3,
            calls: 3,
        };
        assert_eq!(b.usage(), usage);
    }

    #[test]
    fn anthropic_reply_usage_and_stop_reasons() {
        let b = backend("anthropic", "    effort: medium\n");
        let body = b.anthropic_body("sys", "usr");
        assert_eq!(body["system"], "sys");
        assert_eq!(body["output_config"]["effort"], "medium");

        // Text blocks are joined; other blocks are skipped; cache tokens count as input.
        let ok = json!({
            "content": [
                { "type": "thinking", "thinking": "..." },
                { "type": "text", "text": "[1," },
                { "type": "text", "text": "2]" }
            ],
            "stop_reason": "end_turn",
            "usage": { "input_tokens": 10, "cache_read_input_tokens": 90,
                       "cache_creation_input_tokens": 5, "output_tokens": 8 }
        });
        assert_eq!(b.read_anthropic(&ok).unwrap(), "[1,2]");
        let usage = Usage {
            input: 105,
            output: 8,
            calls: 1,
        };
        assert_eq!(b.usage(), usage);

        let cut =
            json!({ "content": [{ "type": "text", "text": "[{" }], "stop_reason": "max_tokens" });
        assert_eq!(cut_off(b.read_anthropic(&cut)).partial, "[{");

        let refused = json!({ "content": [], "stop_reason": "refusal" });
        let err = b.read_anthropic(&refused).unwrap_err().to_string();
        assert!(err.starts_with("the model refused"));
    }

    #[test]
    fn config_rules() {
        // Ollama: an old `/v1` address becomes the server root; the config wins over the default.
        let b = backend("ollama", "    base_url: http://gpu-box:11434/v1/\n");
        assert_eq!(b.base_url, "http://gpu-box:11434");

        let missing = config(
            "active: c\nproviders:\n  c:\n    type: anthropic\n    model: m\n    \
             api_key_env: JOB_RADAR_TEST_KEY_THAT_IS_NOT_SET\n",
        );
        let err = Backend::from_config(&missing).err().unwrap().to_string();
        assert_eq!(
            err,
            "provider 'c' requires the JOB_RADAR_TEST_KEY_THAT_IS_NOT_SET environment variable"
        );
    }

    #[test]
    fn page_budget_keeps_space_for_reply_and_prompt() {
        let b = backend("ollama", "");
        // window 1000 * 0.9 = 900 tokens, minus 100 reply, minus 300 / 3 = 100 prompt tokens.
        assert_eq!(b.page_char_budget(300), (900 - 100 - 100) * CHARS_PER_TOKEN);
        assert_eq!(b.page_char_budget(1_000_000), 0);
    }

    #[test]
    fn strips_think_block_only_when_complete() {
        assert_eq!(strip_think("<think>a</think> [1]"), "[1]");
        assert_eq!(strip_think("<think>a [1]"), "<think>a [1]");
    }
}
