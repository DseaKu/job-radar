use std::collections::BTreeMap;
use std::env;
use std::error::Error;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};

use crate::Res;
use crate::config::{Config, ProviderKind};

const ANTHROPIC_VERSION: &str = "2023-06-01";
const DEFAULT_MAX_TOKENS: u32 = 4096;
/// Upper limit for opening the connection. A reachable API needs well under a second.
const CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);
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

/// The provider rejected the API key (HTTP 401 or 403). All further requests fail the same
/// way, so a search stops at the first one.
#[derive(Debug)]
pub struct KeyRejected(String);

impl std::fmt::Display for KeyRejected {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for KeyRejected {}

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
    /// The environment variable of the API key, if the provider needs one.
    key_var: Option<String>,
    model: String,
    max_tokens: u32,
    effort: Option<String>,
    context_length: u32,
    temperature: Option<f32>,
    /// Extra top-level fields for each request body (`extra_body` in the config).
    extra_body: BTreeMap<String, Value>,
    /// Upper limit in seconds for one request.
    timeout: u32,
    /// openai: read the reply as a stream (server-sent events).
    stream: bool,
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

        let key_var = Config::api_key_var(p).map(str::to_string);
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

        let timeout = p.timeout.unwrap_or(p.kind.default_timeout()).max(1);
        Ok(Self {
            name: name.to_string(),
            kind: p.kind,
            http: reqwest::Client::builder()
                .connect_timeout(CONNECT_TIMEOUT)
                .timeout(std::time::Duration::from_secs(timeout.into()))
                .build()?,
            timeout,
            stream: p.stream.unwrap_or(false),
            base_url,
            api_key,
            key_var,
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
        self.read_ollama(&self.send(request).await?)
    }

    async fn chat_openai(&self, system: &str, user: &str) -> Res<String> {
        let mut request = self
            .http
            .post(format!("{}/chat/completions", self.base_url))
            .json(&self.openai_body(system, user));
        if let Some(key) = &self.api_key {
            request = request.bearer_auth(key);
        }
        if self.stream {
            return self.read_openai_stream(request).await;
        }
        self.read_openai(&self.send(request).await?)
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
        self.read_anthropic(&self.send(request).await?)
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
        if self.stream {
            body["stream"] = json!(true);
        }
        merge_extra(&mut body, &self.extra_body);
        body
    }

    /// Sends a streaming request and joins the parts of the reply.
    async fn read_openai_stream(&self, request: reqwest::RequestBuilder) -> Res<String> {
        let mut response = request.send().await.map_err(|e| self.request_error(&e))?;
        let status = response.status();
        if !status.is_success() {
            let text = response.text().await.map_err(|e| self.request_error(&e))?;
            return Err(self.http_error(status, &text));
        }
        let mut stream = StreamReply::default();
        let mut buf: Vec<u8> = Vec::new();
        'read: while let Some(chunk) = response.chunk().await.map_err(|e| self.request_error(&e))? {
            buf.extend_from_slice(&chunk);
            // Only complete lines: a chunk can end inside a line or inside a UTF-8 character.
            while let Some(end) = buf.iter().position(|&b| b == b'\n') {
                let line: Vec<u8> = buf.drain(..=end).collect();
                if stream.line(&String::from_utf8_lossy(&line), &self.name)? {
                    break 'read;
                }
            }
        }
        if !buf.is_empty() {
            stream.line(&String::from_utf8_lossy(&buf), &self.name)?;
        }
        self.record_usage(
            &stream.usage["prompt_tokens"],
            &stream.usage["completion_tokens"],
        );
        if stream.finish_reason.as_deref() == Some("length") {
            return Err(self.cut_off(&stream.content));
        }
        if stream.content.is_empty() && stream.finish_reason.is_none() {
            return Err(format!("{}: the stream ended without a reply", self.name).into());
        }
        Ok(strip_think(&stream.content))
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

impl Backend {
    /// Sends the request and returns the JSON body, or an error with the API's message, the
    /// cause and a hint.
    async fn send(&self, request: reqwest::RequestBuilder) -> Res<Value> {
        let response = request.send().await.map_err(|e| self.request_error(&e))?;
        let status = response.status();
        let text = response.text().await.map_err(|e| self.request_error(&e))?;
        // An error page from a proxy (for example Cloudflare) is HTML, not JSON.
        let payload: Value = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(_) if !status.is_success() => Value::String(preview(&text)),
            Err(e) => {
                return Err(format!(
                    "{}: the API reply is not valid JSON ({e}): {}",
                    self.name,
                    preview(&text)
                )
                .into());
            }
        };
        if !status.is_success() {
            return Err(self.status_error(status, &payload));
        }
        Ok(payload)
    }

    /// The error for an HTTP error status with the reply text (JSON or an HTML page).
    fn http_error(&self, status: reqwest::StatusCode, text: &str) -> Box<dyn Error + Send + Sync> {
        let payload: Value =
            serde_json::from_str(text).unwrap_or_else(|_| Value::String(preview(text)));
        self.status_error(status, &payload)
    }

    /// The error for an HTTP error status and the parsed reply. 401 and 403 give [`KeyRejected`].
    fn status_error(
        &self,
        status: reqwest::StatusCode,
        payload: &Value,
    ) -> Box<dyn Error + Send + Sync> {
        if matches!(status.as_u16(), 401 | 403) {
            return Box::new(self.key_rejected(status, payload));
        }
        format!(
            "API request failed ({status}): {payload}{}",
            status_hint(status.as_u16(), &self.name)
        )
        .into()
    }

    /// A short message for a rejected API key: which key, the provider's reason, what to do.
    fn key_rejected(&self, status: reqwest::StatusCode, payload: &Value) -> KeyRejected {
        let reason = match &payload["error"]["message"] {
            Value::String(m) => m.clone(),
            _ => match payload {
                Value::String(text) => text.clone(),
                other => other.to_string(),
            },
        };
        let name = &self.name;
        let Some(var) = &self.key_var else {
            return KeyRejected(format!(
                "{name} needs an API key ({status}: {reason}). Set api_key_env for this \
                 provider in config.yaml and put the key in .env."
            ));
        };
        let todo = if status.as_u16() == 403 {
            "The key has no access to this API or model: check the key and the model name."
        } else {
            "Make a new key at the provider and put it in .env (app: Settings > AI provider)."
        };
        KeyRejected(format!(
            "{name} rejected the API key in {var} ({status}: {reason}). {todo}"
        ))
    }

    /// A readable message for a request without an HTTP reply: timeout, no connection and so on.
    /// reqwest's own message ("error sending request") hides the cause, so the causes are added.
    fn request_error(&self, e: &reqwest::Error) -> String {
        let cause = crate::error_causes(e);
        let name = &self.name;
        if e.is_timeout() {
            format!(
                "{name}: no reply within {} seconds (timeout). The provider is slow or overloaded: \
                 try again later. For a slow model, increase timeout in config.yaml \
                 (app: Settings > AI provider > Advanced).",
                self.timeout
            )
        } else if e.is_connect() {
            let hint = if self.kind == ProviderKind::Ollama {
                " Is Ollama running? Start it with: ollama serve"
            } else {
                " Check the base URL and your network."
            };
            format!(
                "{name}: cannot connect to {} ({cause}).{hint}",
                self.base_url
            )
        } else {
            format!("{name}: the request failed ({cause}).")
        }
    }
}

/// The reply of an OpenAI-compatible stream (server-sent events), joined line by line.
#[derive(Default)]
struct StreamReply {
    content: String,
    finish_reason: Option<String>,
    usage: Value,
}

impl StreamReply {
    /// Reads one line of the stream. Returns true at the end (`data: [DONE]`).
    /// An error event in the stream becomes an error.
    fn line(&mut self, line: &str, name: &str) -> Res<bool> {
        let Some(data) = line.trim().strip_prefix("data:") else {
            return Ok(false); // empty lines, comments (": ping") and other fields
        };
        let data = data.trim();
        if data == "[DONE]" {
            return Ok(true);
        }
        let Ok(event) = serde_json::from_str::<Value>(data) else {
            return Ok(false);
        };
        if !event["error"].is_null() {
            let hint = match event["error"]["code"].as_u64() {
                Some(code) => status_hint(code as u16, name),
                None => String::new(),
            };
            return Err(format!("API error in the stream: {}{hint}", event["error"]).into());
        }
        let choice = &event["choices"][0];
        if let Some(text) = choice["delta"]["content"].as_str() {
            self.content.push_str(text);
        }
        if let Some(reason) = choice["finish_reason"].as_str() {
            self.finish_reason = Some(reason.to_string());
        }
        if event["usage"].is_object() {
            self.usage = event["usage"].clone();
        }
        Ok(false)
    }
}

/// A hint for an HTTP error status, or "".
fn status_hint(status: u16, name: &str) -> String {
    let hint = match status {
        401 | 403 => "the API key is wrong or has no access; check it in .env",
        404 => "the base URL or the model name is wrong",
        408 | 504 => "the provider did not answer in time; try again later",
        524 => {
            "the provider needed more than about 100 seconds and its proxy (Cloudflare) stopped \
             the request; a smaller context_length makes each request smaller and faster"
        }
        429 => "too many requests or no credit left; wait a moment or check your account",
        500..=599 => "the provider has a problem; try again later or use another provider",
        _ => return String::new(),
    };
    format!(" (hint for {name}: {hint})")
}

fn preview(text: &str) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    flat.chars().take(300).collect()
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

    /// A one-shot local HTTP server: it answers the first request with `reply` after `delay`.
    async fn server(reply: &'static str, delay: u64) -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 4096];
            let _ = socket.read(&mut buf).await;
            tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
            let _ = socket.write_all(reply.as_bytes()).await;
        });
        format!("http://{addr}")
    }

    fn backend_at(url: &str, extra: &str) -> Backend {
        backend("openai", &format!("    base_url: {url}\n{extra}"))
    }

    #[tokio::test]
    async fn timeout_and_connect_errors_are_clear() {
        let url = server("", 5).await;
        let err = backend_at(&url, "    timeout: 1\n")
            .chat("s", "u")
            .await
            .unwrap_err()
            .to_string();
        assert!(
            err.starts_with("p: no reply within 1 seconds (timeout)"),
            "{err}"
        );

        // Nothing listens on port 1.
        let err = backend_at("http://127.0.0.1:1", "")
            .chat("s", "u")
            .await
            .unwrap_err()
            .to_string();
        assert!(
            err.starts_with("p: cannot connect to http://127.0.0.1:1 ("),
            "{err}"
        );
        assert!(
            err.ends_with("Check the base URL and your network."),
            "{err}"
        );
        assert!(!err.contains("error sending request for url"), "{err}");

        assert_eq!(backend("ollama", "").timeout, 600);
        assert_eq!(backend("anthropic", "").timeout, 300);
    }

    #[tokio::test]
    async fn http_errors_have_a_hint_also_with_an_html_body() {
        let url = server(
            "HTTP/1.1 503 Service Unavailable\r\ncontent-type: text/html\r\ncontent-length: 27\r\n\r\n<html>\n  overloaded\n</html>",
            0,
        )
        .await;
        let err = backend_at(&url, "")
            .chat("s", "u")
            .await
            .unwrap_err()
            .to_string();
        assert_eq!(
            err,
            "API request failed (503 Service Unavailable): \"<html> overloaded </html>\" \
             (hint for p: the provider has a problem; try again later or use another provider)"
        );
        assert!(status_hint(401, "p").contains(".env"));
        assert!(status_hint(524, "p").contains("context_length"));
        assert_eq!(status_hint(400, "p"), "");
    }

    #[tokio::test]
    async fn rejected_key_gives_a_short_message() {
        let url = server(
            "HTTP/1.1 401 Unauthorized\r\ncontent-type: application/json\r\ncontent-length: 86\r\n\r\n\
             {\"type\":\"error\",\"error\":{\"type\":\"authentication_error\",\"message\":\"invalid x-api-key\"}}",
            0,
        )
        .await;
        let err = backend_at(&url, "").chat("s", "u").await.unwrap_err();
        assert!(err.is::<KeyRejected>());
        assert_eq!(
            err.to_string(),
            "p rejected the API key in PATH (401 Unauthorized: invalid x-api-key). \
             Make a new key at the provider and put it in .env (app: Settings > AI provider)."
        );
    }

    #[tokio::test]
    async fn stream_reply_is_joined() {
        // Two lines, one of them with a UTF-8 character; a ping comment; usage; [DONE].
        let url = server(
            "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\n\r\n\
             : ping\n\n\
             data: {\"choices\":[{\"delta\":{\"role\":\"assistant\"},\"finish_reason\":null}]}\n\n\
             data: {\"choices\":[{\"delta\":{\"content\":\"[{\\\"title\\\": \"},\"finish_reason\":null}]}\n\n\
             data: {\"choices\":[{\"delta\":{\"content\":\"\\\"Entwickler äöü\\\"}]\"},\"finish_reason\":\"stop\"}]}\n\n\
             data: {\"choices\":[],\"usage\":{\"prompt_tokens\":12,\"completion_tokens\":7}}\n\n\
             data: [DONE]\n\n",
            0,
        )
        .await;
        let b = backend_at(&url, "    stream: true\n");
        assert_eq!(b.openai_body("s", "u")["stream"], true);
        let reply = b.chat("s", "u").await.unwrap();
        assert_eq!(reply, "[{\"title\": \"Entwickler äöü\"}]");
        assert_eq!(b.usage().input, 12);
        assert_eq!(b.usage().output, 7);

        // Without stream, the body has no stream field.
        assert!(
            backend_at(&url, "")
                .openai_body("s", "u")
                .get("stream")
                .is_none()
        );
    }

    #[test]
    fn stream_errors_and_cut_off() {
        let mut s = StreamReply::default();
        let err = s
            .line(
                r#"data: {"error":{"code":503,"message":"The service could not complete this request."}}"#,
                "p",
            )
            .unwrap_err()
            .to_string();
        assert!(err.starts_with("API error in the stream: {"), "{err}");
        assert!(err.contains("the provider has a problem"), "{err}");

        let mut s = StreamReply::default();
        s.line(
            r#"data: {"choices":[{"delta":{"content":"[{"},"finish_reason":"length"}]}"#,
            "p",
        )
        .unwrap();
        assert_eq!(s.finish_reason.as_deref(), Some("length"));
        assert!(s.line("data: [DONE]", "p").unwrap());
        assert!(!s.line("event: message", "p").unwrap());
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
