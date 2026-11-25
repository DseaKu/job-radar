//! Creates missing settings files (`config.yaml`, `sources.yaml`, ...) from the examples.
//!
//! The examples are built into the program, so this also works in the Docker image, which has
//! no example files. The examples are copied, not moved: they stay in the repository.

use std::fs;
use std::io::{self, BufRead, IsTerminal, Write};
use std::path::Path;

use job_radar::Res;

/// A settings file with the example to create it from.
pub struct Example {
    /// The default file name.
    pub file: &'static str,
    /// The environment variable that names another file. If it is set, the file is not offered.
    env: &'static str,
    text: &'static str,
    /// What to change in the new file.
    hint: &'static str,
}

pub const CONFIG: Example = Example {
    file: "config.yaml",
    env: "AGENTS_CONFIG",
    text: include_str!("../config.example.yaml"),
    hint: "select the AI provider and the model in it",
};
pub const SOURCES: Example = Example {
    file: "sources.yaml",
    env: "JOB_RADAR_SOURCES",
    text: include_str!("../sources.example.yaml"),
    hint: "put your own search pages in it",
};
pub const IGNORE: Example = Example {
    file: "ignore.yaml",
    env: "JOB_RADAR_IGNORE",
    text: include_str!("../ignore.example.yaml"),
    hint: "add the jobs that you do not want to see again",
};
pub const LETTER: Example = Example {
    file: "letter.yaml",
    env: "JOB_RADAR_LETTER",
    text: include_str!("../letter.example.yaml"),
    hint: "put your sender data and your letter layout in it",
};

/// All files that `init` creates.
const ALL: [&Example; 4] = [&CONFIG, &SOURCES, &IGNORE, &LETTER];

/// The files of `wanted` that are missing in `dir`. A file named by its environment variable
/// is not counted: the user chose it on purpose.
fn missing<'a>(dir: &Path, wanted: &[&'a Example], env: impl Fn(&str) -> bool) -> Vec<&'a Example> {
    wanted
        .iter()
        .filter(|e| !env(e.env) && !dir.join(e.file).exists())
        .copied()
        .collect()
}

fn env_is_set(var: &str) -> bool {
    std::env::var(var).is_ok_and(|v| !v.trim().is_empty())
}

/// Writes each example into `dir` and returns one line for each new file.
fn create(dir: &Path, examples: &[&Example]) -> Res<Vec<String>> {
    examples
        .iter()
        .map(|e| {
            fs::write(dir.join(e.file), e.text)
                .map_err(|err| format!("cannot write {}: {err}", e.file))?;
            Ok(format!("Created {} from the example: {}.", e.file, e.hint))
        })
        .collect()
}

/// "a", "a and b", "a, b and c".
fn names(examples: &[&Example]) -> String {
    let names: Vec<&str> = examples.iter().map(|e| e.file).collect();
    match names.split_last() {
        Some((last, rest)) if !rest.is_empty() => format!("{} and {last}", rest.join(", ")),
        _ => names.join(""),
    }
}

/// True for "y" and "yes" (any case). Empty and all other answers are "no".
fn is_yes(answer: &str) -> bool {
    matches!(answer.trim().to_lowercase().as_str(), "y" | "yes")
}

/// The command that runs `init`: inside the Docker image, the full `docker run` command.
fn init_command() -> &'static str {
    if Path::new("/.dockerenv").exists() {
        "docker run --rm -v \"$PWD:/work\" ghcr.io/dseaku/job-radar init"
    } else {
        "job_radar init"
    }
}

/// If files of `wanted` are missing, asks (y/N) to create them from the examples. Without a
/// terminal, it cannot ask: it shows a hint instead. "No" keeps the built-in defaults.
pub fn offer(wanted: &[&Example]) -> Res<()> {
    let dir = Path::new(".");
    let missing = missing(dir, wanted, env_is_set);
    if missing.is_empty() {
        return Ok(());
    }
    let (names, verb, them) = match missing.len() {
        1 => (names(&missing), "is", "it"),
        _ => (names(&missing), "are", "them"),
    };
    if !(io::stdin().is_terminal() && io::stderr().is_terminal()) {
        eprintln!(
            "hint: {names} {verb} missing. To create {them} from the examples, run: {}",
            init_command()
        );
        return Ok(());
    }

    eprint!("{names} {verb} missing. Create {them} from the examples? [y/N] ");
    io::stderr().flush()?;
    let mut answer = String::new();
    io::stdin().lock().read_line(&mut answer)?;
    if is_yes(&answer) {
        for line in create(dir, &missing)? {
            eprintln!("{line}");
        }
    } else {
        eprintln!(
            "Not created. To create {them} later, run: {}",
            init_command()
        );
    }
    Ok(())
}

/// `job_radar init`: creates all missing settings files from the examples.
pub fn init() -> Res<()> {
    let dir = Path::new(".");
    let missing = missing(dir, &ALL, |_| false);
    for e in ALL
        .iter()
        .filter(|e| !missing.iter().any(|m| m.file == e.file))
    {
        eprintln!("{} exists: not changed.", e.file);
    }
    for line in create(dir, &missing)? {
        eprintln!("{line}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_joined_in_english() {
        assert_eq!(names(&[&CONFIG]), "config.yaml");
        assert_eq!(names(&[&CONFIG, &SOURCES]), "config.yaml and sources.yaml");
        assert_eq!(
            names(&[&CONFIG, &SOURCES, &IGNORE]),
            "config.yaml, sources.yaml and ignore.yaml"
        );
    }

    #[test]
    fn only_yes_creates_the_files() {
        for yes in ["y", "Y", "yes", " YES \n"] {
            assert!(is_yes(yes), "{yes:?}");
        }
        for no in ["", "\n", "n", "no", "ja", "yy"] {
            assert!(!is_yes(no), "{no:?}");
        }
    }

    #[test]
    fn missing_files_are_created_from_the_examples() {
        let dir = std::env::temp_dir().join(format!("job_radar_setup_{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("config.yaml"), "own").unwrap();

        // config.yaml exists; sources.yaml is named by its variable; only ignore.yaml is missing.
        let found = missing(&dir, &[&CONFIG, &SOURCES, &IGNORE], |var| {
            var == "JOB_RADAR_SOURCES"
        });
        assert_eq!(names(&found), "ignore.yaml");

        let lines = create(&dir, &found).unwrap();
        assert_eq!(
            lines,
            [
                "Created ignore.yaml from the example: add the jobs that you do not want to see again."
            ]
        );
        assert_eq!(
            fs::read_to_string(dir.join("ignore.yaml")).unwrap(),
            IGNORE.text
        );
        assert_eq!(fs::read_to_string(dir.join("config.yaml")).unwrap(), "own");
        fs::remove_dir_all(&dir).unwrap();
    }
}
