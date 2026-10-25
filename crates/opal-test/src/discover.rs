//! Which files are test files, by Jest's rules.
//!
//! A file under one of `roots` is a test file when its extension is one of
//! `moduleFileExtensions`, it matches `testMatch` (or `testRegex`, when that
//! is set instead), and no `testPathIgnorePatterns` entry matches its
//! absolute path. Paths given on the command line narrow that further: each
//! is a regular expression, and a file has to match one. Jest compares
//! everything against absolute paths, and so does this.

use std::fs;
use std::path::{Path, PathBuf};

use regex::Regex;

use crate::TestError;
use crate::config::ProjectConfig;
use crate::glob::{Glob, matches_any};

pub fn find_tests(config: &ProjectConfig, filters: &[Regex]) -> Result<Vec<PathBuf>, TestError> {
    let globs = config
        .test_match
        .iter()
        .map(|pattern| Glob::new(pattern))
        .collect::<Result<Vec<_>, _>>()?;
    let regexes = compile(&config.test_regex)?;
    let ignored = compile(&config.test_path_ignore_patterns)?;
    let skip_node_modules = config
        .test_path_ignore_patterns
        .iter()
        .any(|pattern| pattern.contains("node_modules"));

    let mut found = Vec::new();
    for root in &config.roots {
        walk(root, skip_node_modules, &mut |path| {
            let Some(text) = path.to_str() else { return };
            let text = text.replace('\\', "/");
            let extension = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if !config.module_file_extensions.iter().any(|e| e == extension) {
                return;
            }
            let is_test = if globs.is_empty() {
                regexes.iter().any(|r| r.is_match(&text))
            } else {
                matches_any(&globs, &text)
            };
            if !is_test || ignored.iter().any(|r| r.is_match(&text)) {
                return;
            }
            if !filters.is_empty() && !filters.iter().any(|r| r.is_match(&text)) {
                return;
            }
            found.push(path.to_path_buf());
        });
    }
    found.sort();
    found.dedup();
    // Largest first: without timings from an earlier run, size is the best
    // guess at what takes longest, and starting it early shortens the run.
    found.sort_by_key(|path| std::cmp::Reverse(fs::metadata(path).map(|m| m.len()).unwrap_or(0)));
    Ok(found)
}

pub fn compile(patterns: &[String]) -> Result<Vec<Regex>, TestError> {
    patterns
        .iter()
        .map(|pattern| {
            Regex::new(pattern)
                .map_err(|error| TestError::Pattern(pattern.clone(), error.to_string()))
        })
        .collect()
}

fn walk(dir: &Path, skip_node_modules: bool, visit: &mut dyn FnMut(&Path)) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.filter_map(Result::ok).collect();
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let name = entry.file_name();
        if kind.is_dir() {
            if name == ".git" || name == ".hg" || name == ".svn" {
                continue;
            }
            if skip_node_modules && name == "node_modules" {
                continue;
            }
            walk(&path, skip_node_modules, visit);
        } else if kind.is_file() {
            visit(&path);
        }
    }
}
