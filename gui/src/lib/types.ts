// The data types of the Rust side (see src-tauri/src/lib.rs and the job_radar library).

export type ProviderKind = "ollama" | "openai" | "anthropic";

export interface ProviderConfig {
  type: ProviderKind;
  model: string;
  base_url?: string | null;
  api_key_env?: string | null;
  max_tokens?: number | null;
  effort?: string | null;
  context_length?: number | null;
  temperature?: number | null;
  /** Extra top-level fields for the JSON body of each request. */
  extra_body?: Record<string, unknown> | null;
}

export interface Config {
  active: string;
  providers: Record<string, ProviderConfig>;
  output_language: string;
  extra_instructions: string;
  applications_dir: string;
  application_instructions: string;
}

export interface Source {
  name: string;
  url: string;
  enabled: boolean;
}

export interface Rule {
  url?: string | null;
  company?: string | null;
  title?: string | null;
  note?: string | null;
}

export type VarValue = string | number | boolean;

export interface Settings {
  config: Config;
  vars: Record<string, VarValue>;
  sources: Source[];
  ignore: Rule[];
  api_keys: Record<string, string>;
  env_keys: string[];
}

export interface JobRow {
  match_score: number | null;
  title: string;
  company: string;
  location: string;
  description: string;
  match_reason: string;
  url: string;
  source: string;
}

export interface StepUsage {
  name: string;
  input: number;
  output: number;
  calls: number;
}

export interface RunDone {
  rows: JobRow[];
  usage: StepUsage[];
  failed: number;
  error: string | null;
  stopped: boolean;
}

export type Progress =
  | { step: "source"; index: number; total: number; name: string }
  | { step: "source_done"; name: string; jobs: number; error: string | null }
  | { step: "dedup" };

export interface LogLine {
  level: "info" | "warn";
  msg: string;
}

export interface PageTest {
  url: string;
  chars: number;
  preview: string;
}

/**
 * Reads the "extra request body" text of a provider. Empty text gives null (no extra fields).
 * The text must be a JSON object, for example {"mode": "non-private"}.
 */
export function parseExtraBody(text: string): { value: Record<string, unknown> | null } | { error: string } {
  if (!text.trim()) return { value: null };
  let value: unknown;
  try {
    value = JSON.parse(text);
  } catch (e) {
    return { error: `Not valid JSON: ${e instanceof Error ? e.message : String(e)}` };
  }
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    return { error: 'The extra fields must be a JSON object, for example {"mode": "non-private"}.' };
  }
  return { value: Object.keys(value).length > 0 ? (value as Record<string, unknown>) : null };
}

/** The environment variable for the API key of a provider; null if it needs none. */
export function apiKeyVar(p: ProviderConfig): string | null {
  if (p.api_key_env) return p.api_key_env;
  if (p.type === "openai") return "OPENAI_API_KEY";
  if (p.type === "anthropic") return "ANTHROPIC_API_KEY";
  return null;
}

export const DEFAULT_MODEL: Record<ProviderKind, string> = {
  ollama: "qwen3:1.7b",
  openai: "gpt-4o-mini",
  anthropic: "claude-haiku-5-5",
};

export const DEFAULT_BASE_URL: Record<ProviderKind, string> = {
  ollama: "http://localhost:11434",
  openai: "https://api.openai.com/v1",
  anthropic: "https://api.anthropic.com/v1",
};

export const DEFAULT_CONTEXT: Record<ProviderKind, number> = {
  ollama: 32768,
  openai: 128000,
  anthropic: 200000,
};

export const KIND_LABEL: Record<ProviderKind, string> = {
  ollama: "Local (Ollama)",
  openai: "OpenAI-compatible",
  anthropic: "Claude (Anthropic)",
};
