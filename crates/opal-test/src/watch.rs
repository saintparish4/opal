//! `--watch` and `--watchAll`: run, wait for files to change, run again.
//!
//! The workers stay up between runs, and with them what they have compiled.
//! So do the transform cache and the loaded config.
//!
//! - `--watchAll` reruns every test.
//! - `--watch` starts with the tests related to what git says has changed,
//!   as `-o` does. Each rerun then takes the tests related to the files that
//!   changed since the run before. The query is the one `--findRelatedTests`
//!   answers (related.rs), on test files found afresh each time, so a new
//!   test or a new import counts.
//!
//! Changes are gathered until none has come for a moment, so a save that
//! writes several files runs once. As in Jest, only a file the haste map
//! would hold counts: one with an extension in `moduleFileExtensions` (or a
//! snapshot), outside `node_modules`, and matching neither
//! `modulePathIgnorePatterns` nor `watchPathIgnorePatterns`. Jest's
//! interactive prompt (a, f, p, t, q) isn't there; Ctrl+C stops.

use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::Duration;

use notify::event::ModifyKind;
use notify::{EventKind, RecursiveMode, Watcher};
use regex::Regex;

use crate::{
    Outcome, RunOptions, TestError, Watch, changed, discover, load_project, pool, run_files, select,
};

/// How long it has to stay quiet before the changes so far are run.
const QUIET: Duration = Duration::from_millis(100);

type Events = mpsc::Receiver<notify::Result<notify::Event>>;

pub(crate) fn watch(
    pool: &mut pool::Pool,
    options: &RunOptions,
    mode: Watch,
    out: &mut dyn Write,
) -> Result<Outcome, TestError> {
    let project = load_project(pool, options)?;
    let config = &project.config;
    let ignored = discover::compile(
        &[
            &config.watch_path_ignore_patterns[..],
            &config.module_path_ignore_patterns[..],
        ]
        .concat(),
    )?;
    let extensions: Vec<&str> = config
        .module_file_extensions
        .iter()
        .map(String::as_str)
        .chain(["snap"])
        .collect();
    let counts = |path: &Path| relevant(path, &extensions, &ignored);
    let (sender, receiver) = mpsc::channel();
    let mut watcher = notify::recommended_watcher(move |event| {
        let _ = sender.send(event);
    })
    .map_err(|error| TestError::Watch(error.to_string()))?;
    for root in &project.config.roots {
        watcher
            .watch(root, RecursiveMode::Recursive)
            .map_err(|error| TestError::Watch(format!("{}: {error}", root.display())))?;
    }

    let mut changed: Option<Vec<PathBuf>> = match mode {
        Watch::Related => Some(changed::changed_files(&project.config.roots)?),
        _ => None,
    };
    let mut since = "the last commit";
    loop {
        let found = select(options, &project, changed.as_deref())?;
        if found.tests.is_empty() {
            let _ = writeln!(
                out,
                "No tests found related to files changed since {since}."
            );
        } else {
            run_files(pool, options, &project, found, out)?;
        }
        let _ = writeln!(out, "\nWatching for file changes. Press Ctrl+C to stop.");
        let _ = out.flush();
        let paths = next_changes(&receiver, &counts)?;
        changed = match mode {
            Watch::All => None,
            _ => Some(paths),
        };
        since = "the last run";
    }
}

/// Blocks until a relevant file changes, then gathers changes until it has
/// been quiet for `QUIET`.
fn next_changes(
    events: &Events,
    counts: &dyn Fn(&Path) -> bool,
) -> Result<Vec<PathBuf>, TestError> {
    let mut paths = BTreeSet::new();
    loop {
        let event = if paths.is_empty() {
            events
                .recv()
                .map_err(|_| TestError::Watch("the file watcher stopped".to_string()))?
        } else {
            match events.recv_timeout(QUIET) {
                Ok(event) => event,
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(TestError::Watch("the file watcher stopped".to_string()));
                }
            }
        };
        let Ok(event) = event else { continue };
        let is_change = match event.kind {
            EventKind::Create(_) | EventKind::Remove(_) => true,
            EventKind::Modify(kind) => !matches!(kind, ModifyKind::Metadata(_)),
            _ => false,
        };
        if !is_change {
            continue;
        }
        for path in event.paths {
            if counts(&path) {
                paths.insert(path);
            }
        }
    }
    Ok(paths.into_iter().collect())
}

fn relevant(path: &Path, extensions: &[&str], ignored: &[Regex]) -> bool {
    if path
        .components()
        .any(|part| part.as_os_str() == "node_modules" || part.as_os_str() == ".git")
    {
        return false;
    }
    let extension = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    if !extensions.contains(&extension) {
        return false;
    }
    let text = path.to_string_lossy();
    !ignored.iter().any(|pattern| pattern.is_match(&text))
}
