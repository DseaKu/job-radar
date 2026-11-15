//! Finds job postings on career pages and rates them against a CV with an LLM.
//!
//! The command-line program (`src/main.rs`) and the desktop app (`gui/`) use this library.
//! The entry point is [`run::run`]. Messages go through [`log`]: install a receiver with
//! [`set_logger`], otherwise warnings go to stderr and progress messages are dropped.

use std::sync::RwLock;

use serde::Serialize;

pub type Res<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

/// How important a message is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    /// Progress: what the program does now.
    Info,
    /// A problem that does not stop the run.
    Warn,
}

type Logger = Box<dyn Fn(Level, &str) + Send + Sync>;

static LOGGER: RwLock<Option<Logger>> = RwLock::new(None);

/// Sets the receiver for all messages of the library. It replaces the previous one.
pub fn set_logger(logger: impl Fn(Level, &str) + Send + Sync + 'static) {
    if let Ok(mut slot) = LOGGER.write() {
        *slot = Some(Box::new(logger));
    }
}

/// Sends a message to the receiver from [`set_logger`]. Without one, warnings go to stderr.
pub fn log(level: Level, msg: &str) {
    match LOGGER.read().as_deref() {
        Ok(Some(logger)) => logger(level, msg),
        _ if level == Level::Warn => eprintln!("{msg}"),
        _ => {}
    }
}

/// Progress message, see [`log`].
macro_rules! info {
    ($($arg:tt)*) => {
        $crate::log($crate::Level::Info, &format!($($arg)*))
    };
}

/// Warning, see [`log`].
macro_rules! warn {
    ($($arg:tt)*) => {
        $crate::log($crate::Level::Warn, &format!($($arg)*))
    };
}

pub mod application;
pub mod backend;
pub mod config;
pub mod cv;
pub mod dedup;
pub mod dump;
pub mod fetch;
pub mod ignore;
pub mod jobs;
pub mod run;

/// The HTTP client for reading the job pages and for the AI APIs.
pub fn http_client() -> Res<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (compatible; job_radar/0.1)")
        .timeout(std::time::Duration::from_secs(30))
        .build()?)
}
