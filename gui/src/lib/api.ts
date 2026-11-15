// Typed wrappers for the Tauri commands in src-tauri/src/lib.rs.

import { invoke } from "@tauri-apps/api/core";
import type { Config, JobRow, PageTest, Rule, Settings, VarValue } from "./types";

export const appInfo = () => invoke<{ data_dir: string }>("app_info");

export const cvInfo = (path: string) => invoke<{ words: number }>("cv_info", { path });

export const loadSettings = () => invoke<Settings>("load_settings");

export const saveSettings = (settings: Settings) => invoke<void>("save_settings", { settings });

export const addIgnoreRule = (rule: Rule) => invoke<Rule[]>("add_ignore_rule", { rule });

export const testConnection = (config: Config, apiKeys: Record<string, string>) =>
  invoke<string>("test_connection", { config, apiKeys });

export const testPage = (url: string, vars: Record<string, VarValue>) =>
  invoke<PageTest>("test_page", { url, vars });

export const ollamaModels = (baseUrl: string | null) =>
  invoke<string[]>("ollama_models", { baseUrl });

export const startRun = (cvPath: string, killDups: boolean) =>
  invoke<void>("start_run", { cvPath, killDups });

export const stopRun = () => invoke<void>("stop_run");

export const openCsv = (path: string) => invoke<JobRow[]>("open_csv", { path });

export const saveCsv = (path: string, rows: JobRow[]) => invoke<void>("save_csv", { path, rows });

/** The error text of a failed command. */
export function errorText(e: unknown): string {
  return typeof e === "string" ? e : e instanceof Error ? e.message : JSON.stringify(e);
}

/** Small preferences of the app itself (not in the YAML files). */
export const prefs = {
  get(key: string): string | null {
    try {
      return localStorage.getItem(`job-radar.${key}`);
    } catch {
      return null;
    }
  },
  set(key: string, value: string | null) {
    try {
      if (value === null) localStorage.removeItem(`job-radar.${key}`);
      else localStorage.setItem(`job-radar.${key}`, value);
    } catch {
      /* not important */
    }
  },
};
