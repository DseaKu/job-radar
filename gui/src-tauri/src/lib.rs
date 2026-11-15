//! Desktop app for job_radar: Tauri commands around the job_radar library.
//!
//! The app works in one data folder, the same way the CLI works in its current folder:
//! `config.yaml`, `sources.yaml`, `ignore.yaml` and `.env` are read from and written to it.
//! - `JOB_RADAR_DIR` set: that folder.
//! - Debug build (`npm run tauri dev`): the repository root, so the CLI and the app share files.
//! - Release build: the app data folder of the operating system.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use job_radar::backend::{Backend, Usage};
use job_radar::config::{self, Config, Source, SourcesFile};
use job_radar::ignore::{self, Rule};
use job_radar::jobs::{self, JobRow};
use job_radar::{Level, cv, fetch, run};
use serde::{Deserialize, Serialize};
use tauri::async_runtime::JoinHandle;
use tauri::{AppHandle, Emitter, Manager, State};

/// The file with the API keys, in the data folder.
const ENV_FILE: &str = ".env";

type CmdRes<T> = Result<T, String>;

fn text(e: impl std::fmt::Display) -> String {
    e.to_string()
}

// ---------------------------------------------------------------------------------------------
// Data folder and .env file

fn data_dir(app: &tauri::App) -> PathBuf {
    if let Some(dir) = std::env::var_os("JOB_RADAR_DIR").filter(|d| !d.is_empty()) {
        return dir.into();
    }
    if cfg!(debug_assertions) {
        return Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    }
    app.path()
        .app_data_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
}

/// The variables in the `.env` file. A missing file gives an empty map.
fn read_env_file() -> BTreeMap<String, String> {
    match dotenvy::from_path_iter(ENV_FILE) {
        Ok(iter) => iter.filter_map(Result::ok).collect(),
        Err(_) => BTreeMap::new(),
    }
}

/// An API key: from the `.env` file first, then from the environment of the process.
fn key_lookup(env_file: &BTreeMap<String, String>, var: &str) -> Option<String> {
    env_file
        .get(var)
        .filter(|v| !v.trim().is_empty())
        .cloned()
        .or_else(|| std::env::var(var).ok())
}

/// A value for a `.env` line, quoted if necessary, so that dotenvy reads it back unchanged.
fn env_value(value: &str) -> String {
    let safe = |c: char| c.is_ascii_alphanumeric() || "-_.:/+=@,".contains(c);
    if value.chars().all(safe) {
        value.to_string()
    } else if !value.contains('\'') {
        // Single quotes: dotenvy reads the text literally.
        format!("'{value}'")
    } else {
        let escaped = value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('$', "\\$");
        format!("\"{escaped}\"")
    }
}

/// The new text of a `.env` file: the lines of `keys` are replaced, added or (for an empty
/// value) removed. All other lines stay unchanged.
fn update_env_text(old: &str, keys: &BTreeMap<String, String>) -> String {
    let line_name = |line: &str| -> Option<String> {
        let line = line.trim_start();
        if line.starts_with('#') {
            return None;
        }
        let line = line.strip_prefix("export ").unwrap_or(line);
        let (name, _) = line.split_once('=')?;
        Some(name.trim().to_string())
    };
    let mut done = BTreeSet::new();
    let mut out = String::new();
    for line in old.lines() {
        if let Some(name) = line_name(line)
            && let Some(value) = keys.get(&name)
        {
            let value = value.trim();
            if done.insert(name.clone()) && !value.is_empty() {
                out += &format!("{name}={}\n", env_value(value));
            }
            continue;
        }
        out += line;
        out.push('\n');
    }
    for (name, value) in keys {
        let value = value.trim();
        if !done.contains(name) && !value.is_empty() {
            out += &format!("{name}={}\n", env_value(value));
        }
    }
    out
}

fn write_env_keys(keys: &BTreeMap<String, String>) -> CmdRes<()> {
    let old = fs::read_to_string(ENV_FILE).unwrap_or_default();
    let new = update_env_text(&old, keys);
    if new == old {
        return Ok(());
    }
    fs::write(ENV_FILE, new).map_err(|e| format!("cannot write {ENV_FILE}: {e}"))?;
    // The file holds secrets: only the owner can read it.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(ENV_FILE, fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Commands: app and CV

#[derive(Serialize)]
struct AppInfo {
    data_dir: String,
}

#[tauri::command]
fn app_info() -> CmdRes<AppInfo> {
    let dir = std::env::current_dir().map_err(text)?;
    Ok(AppInfo {
        data_dir: dir.display().to_string(),
    })
}

fn read_cv(path: &str) -> job_radar::Res<String> {
    let cv = cv::read(path)?;
    if cv.trim().is_empty() {
        return Err(format!(
            "{path} has no text. A scanned PDF has no text layer: use a text file instead."
        )
        .into());
    }
    Ok(cv)
}

#[derive(Serialize)]
struct CvInfo {
    words: usize,
}

#[tauri::command]
async fn cv_info(path: String) -> CmdRes<CvInfo> {
    let cv = read_cv(&path).map_err(text)?;
    Ok(CvInfo {
        words: cv.split_whitespace().count(),
    })
}

// ---------------------------------------------------------------------------------------------
// Commands: settings

/// All settings that the settings screen edits.
#[derive(Serialize, Deserialize)]
struct Settings {
    config: Config,
    /// The variables of sources.yaml (`{{ name }}` in a URL).
    vars: BTreeMap<String, serde_yaml::Value>,
    sources: Vec<Source>,
    ignore: Vec<Rule>,
    /// API key variable -> value, from the `.env` file.
    api_keys: BTreeMap<String, String>,
    /// API key variables that are set in the environment of the app, but not in `.env`.
    env_keys: Vec<String>,
}

#[tauri::command]
fn load_settings() -> CmdRes<Settings> {
    let config = Config::load().map_err(text)?;
    let file = config::load_sources_file().map_err(text)?;
    let ignore = ignore::load().map_err(text)?;
    let env_file = read_env_file();
    let mut api_keys = BTreeMap::new();
    let mut env_keys = Vec::new();
    for provider in config.providers.values() {
        if let Some(var) = Config::api_key_var(provider) {
            match env_file.get(var) {
                Some(value) => {
                    api_keys.insert(var.to_string(), value.clone());
                }
                None if std::env::var(var).is_ok_and(|v| !v.trim().is_empty()) => {
                    env_keys.push(var.to_string());
                }
                None => {}
            }
        }
    }
    Ok(Settings {
        config,
        vars: file.vars,
        sources: file.sources,
        ignore,
        api_keys,
        env_keys,
    })
}

#[tauri::command]
fn save_settings(settings: Settings) -> CmdRes<()> {
    // Check everything first, so that an error does not leave half-written files.
    let Settings {
        config,
        vars,
        sources,
        ignore: rules,
        api_keys,
        env_keys: _,
    } = settings;
    let (_, active) = config.active_provider().map_err(text)?;
    if active.model.trim().is_empty() {
        return Err(format!("The provider '{}' needs a model.", config.active));
    }
    if let Some(i) = sources
        .iter()
        .position(|s| s.name.trim().is_empty() || s.url.trim().is_empty())
    {
        return Err(format!("Source {} needs a name and a URL.", i + 1));
    }
    let file = SourcesFile { vars, sources };
    file.clone().expand().map_err(|e| format!("Sources: {e}"))?;

    // Only the keys of the configured providers go into .env.
    let used: BTreeSet<&str> = config
        .providers
        .values()
        .filter_map(Config::api_key_var)
        .collect();
    let keys: BTreeMap<String, String> = api_keys
        .into_iter()
        .filter(|(var, _)| used.contains(var.as_str()))
        .collect();

    config.save().map_err(text)?;
    config::save_sources_file(&file).map_err(text)?;
    ignore::save(&rules).map_err(text)?;
    write_env_keys(&keys)
}

/// Adds one rule to the ignore list file and returns the new list.
#[tauri::command]
fn add_ignore_rule(rule: Rule) -> CmdRes<Vec<Rule>> {
    if rule.is_empty() {
        return Err("The rule needs a URL, a company or a title.".into());
    }
    let mut rules = ignore::load().map_err(text)?;
    if !rules.contains(&rule) {
        rules.push(rule);
        ignore::save(&rules).map_err(text)?;
    }
    Ok(rules)
}

/// Sends one short request to the provider and returns its reply.
#[tauri::command]
async fn test_connection(config: Config, api_keys: BTreeMap<String, String>) -> CmdRes<String> {
    let backend =
        Backend::from_config_with_keys(&config, |var| key_lookup(&api_keys, var)).map_err(text)?;
    let reply = backend
        .chat(
            "You check if an AI API works. Reply with only the word OK.",
            "Check",
        )
        .await
        .map_err(text)?;
    let reply: String = reply.trim().chars().take(80).collect();
    Ok(format!("{} replied: {reply}", backend.describe()))
}

#[derive(Serialize)]
struct PageTest {
    url: String,
    chars: usize,
    preview: String,
}

/// Reads a page without AI and returns the start of the text that the AI would get.
#[tauri::command]
async fn test_page(url: String, vars: BTreeMap<String, serde_yaml::Value>) -> CmdRes<PageTest> {
    let url = config::expand_vars(&url, &vars).map_err(text)?;
    let http = job_radar::http_client().map_err(text)?;
    let page = fetch::page_text(&http, &url).await.map_err(text)?;
    Ok(PageTest {
        chars: page.chars().count(),
        preview: page.chars().take(1500).collect(),
        url,
    })
}

/// The models that are installed in Ollama.
#[tauri::command]
async fn ollama_models(base_url: Option<String>) -> CmdRes<Vec<String>> {
    let base = base_url
        .filter(|u| !u.trim().is_empty())
        .unwrap_or_else(|| "http://localhost:11434".to_string());
    let base = base.trim_end_matches('/');
    let base = base.strip_suffix("/v1").unwrap_or(base);
    let reply: serde_json::Value = reqwest::get(format!("{base}/api/tags"))
        .await
        .map_err(|e| format!("Ollama is not reachable at {base}: {e}"))?
        .json()
        .await
        .map_err(text)?;
    let mut names: Vec<String> = reply["models"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| m["name"].as_str().map(String::from))
        .collect();
    names.sort();
    Ok(names)
}

// ---------------------------------------------------------------------------------------------
// Commands: search and results

/// The running search, if there is one.
#[derive(Default)]
struct RunState(Mutex<Option<JoinHandle<()>>>);

#[derive(Clone, Serialize)]
struct StepUsage {
    name: String,
    #[serde(flatten)]
    usage: Usage,
}

/// Sent as the `run-done` event at the end of a search.
#[derive(Clone, Default, Serialize)]
struct RunDone {
    rows: Vec<JobRow>,
    usage: Vec<StepUsage>,
    failed: usize,
    error: Option<String>,
    stopped: bool,
}

#[derive(Clone, Serialize)]
struct LogLine {
    level: Level,
    msg: String,
}

async fn search(app: &AppHandle, cv_path: &str, kill_dups: bool) -> job_radar::Res<run::Outcome> {
    let cv = read_cv(cv_path)?;
    let config = Config::load()?;
    let rules = ignore::load()?;
    let targets = run::targets(&[], config::load_sources)?;
    let env_file = read_env_file();
    let backend = Backend::from_config_with_keys(&config, |var| key_lookup(&env_file, var))?;
    let http = job_radar::http_client()?;
    let opts = run::Options {
        cv: &cv,
        targets: &targets,
        kill_dups,
        dump: None,
    };
    let events = app.clone();
    run::run(&backend, &config, &rules, &opts, &http, move |p| {
        let _ = events.emit("progress", p);
    })
    .await
}

/// Starts a search in the background. Progress comes as `progress` and `log` events,
/// the result as one `run-done` event.
#[tauri::command]
fn start_run(
    app: AppHandle,
    state: State<'_, RunState>,
    cv_path: String,
    kill_dups: bool,
) -> CmdRes<()> {
    let mut slot = state.0.lock().map_err(text)?;
    if slot.is_some() {
        return Err("A search is already running.".into());
    }
    let handle = app.clone();
    *slot = Some(tauri::async_runtime::spawn(async move {
        let done = match search(&handle, &cv_path, kill_dups).await {
            Ok(outcome) => RunDone {
                rows: outcome.rows,
                usage: outcome
                    .usage
                    .into_iter()
                    .map(|(name, usage)| StepUsage { name, usage })
                    .collect(),
                failed: outcome.failed,
                ..RunDone::default()
            },
            Err(e) => RunDone {
                error: Some(e.to_string()),
                ..RunDone::default()
            },
        };
        if let Ok(mut slot) = handle.state::<RunState>().0.lock() {
            *slot = None;
        }
        let _ = handle.emit("run-done", done);
    }));
    Ok(())
}

#[tauri::command]
fn stop_run(app: AppHandle, state: State<'_, RunState>) -> CmdRes<()> {
    if let Some(task) = state.0.lock().map_err(text)?.take() {
        task.abort();
        let _ = app.emit(
            "run-done",
            RunDone {
                stopped: true,
                ..RunDone::default()
            },
        );
    }
    Ok(())
}

#[tauri::command]
fn open_csv(path: String) -> CmdRes<Vec<JobRow>> {
    jobs::read_csv(&path).map_err(text)
}

#[tauri::command]
fn save_csv(path: String, mut rows: Vec<JobRow>) -> CmdRes<()> {
    jobs::write_csv(&path, &mut rows).map_err(text)
}

// ---------------------------------------------------------------------------------------------

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(RunState::default())
        .setup(|app| {
            let dir = data_dir(app);
            fs::create_dir_all(&dir)?;
            std::env::set_current_dir(dir.canonicalize()?)?;

            // All messages of the library go to the window.
            let handle = app.handle().clone();
            job_radar::set_logger(move |level, msg| {
                let line = LogLine {
                    level,
                    msg: msg.trim_end().to_string(),
                };
                let _ = handle.emit("log", line);
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_info,
            cv_info,
            load_settings,
            save_settings,
            add_ignore_rule,
            test_connection,
            test_page,
            ollama_models,
            start_run,
            stop_run,
            open_csv,
            save_csv,
        ])
        .run(tauri::generate_context!())
        .expect("error while running the Job Radar app");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_file_update_keeps_other_lines_and_reads_back() {
        let old = "# my keys\nOTHER=1\nANTHROPIC_API_KEY=old\nexport OPENAI_API_KEY=x\n";
        let keys = BTreeMap::from([
            ("ANTHROPIC_API_KEY".to_string(), "sk-ant-new".to_string()),
            ("OPENAI_API_KEY".to_string(), String::new()),
            ("OPENROUTER_API_KEY".to_string(), "a b'c\"$d\\".to_string()),
        ]);
        let new = update_env_text(old, &keys);
        assert!(new.starts_with("# my keys\nOTHER=1\nANTHROPIC_API_KEY=sk-ant-new\n"));
        assert!(!new.contains("OPENAI_API_KEY"));

        let read: BTreeMap<String, String> = dotenvy::from_read_iter(new.as_bytes())
            .map(Result::unwrap)
            .collect();
        assert_eq!(read["OTHER"], "1");
        assert_eq!(read["ANTHROPIC_API_KEY"], "sk-ant-new");
        assert_eq!(read["OPENROUTER_API_KEY"], "a b'c\"$d\\");
        assert_eq!(env_value("with space"), "'with space'");
    }
}
