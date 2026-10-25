//! Which files are test files, by Jest's rules.
//!
//! Jest sees a project through jest-haste-map: every file under one of
//! `roots` whose extension is one of `moduleFileExtensions` (or `snap`),
//! outside `node_modules`, and not matching `modulePathIgnorePatterns`. Of
//! those, a test file matches `testMatch` (or `testRegex`, when that is set
//! instead), and no `testPathIgnorePatterns` entry matches its absolute path.
//! Paths given on the command line narrow that further: each is a regular
//! expression, and a file has to match one. Jest compares everything against
//! absolute paths, and so does this.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use regex::Regex;

use crate::TestError;
use crate::config::ProjectConfig;
use crate::glob::{Glob, matches_any};

/// The test files to run, and jest-haste-map's manual mocks: every file in a
/// `__mocks__` folder under the roots, by its path inside that folder without
/// the extension (`__mocks__/fs.js` is `fs`, `__mocks__/@scope/pkg.js` is
/// `@scope/pkg`). When two share a name the later one in the walk wins, as
/// the last one Jest's crawler reports does. `files` is everything the haste
/// map holds, which is what a related-tests query searches.
pub struct Found {
    pub tests: Vec<PathBuf>,
    pub manual_mocks: BTreeMap<String, PathBuf>,
    pub files: Vec<PathBuf>,
}

pub fn find_tests(config: &ProjectConfig, filters: &[Regex]) -> Result<Found, TestError> {
    let globs = config
        .test_match
        .iter()
        .map(|pattern| Glob::new(pattern))
        .collect::<Result<Vec<_>, _>>()?;
    let regexes = compile(&config.test_regex)?;
    let ignored = compile(&config.test_path_ignore_patterns)?;
    let haste_ignored = compile(&config.module_path_ignore_patterns)?;

    let mut found = Vec::new();
    let mut manual_mocks = BTreeMap::new();
    let mut files = Vec::new();
    for root in &config.roots {
        walk(root, &mut |path| {
            let Some(text) = path.to_str() else { return };
            let text = text.replace('\\', "/");
            let extension = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            let is_module = config.module_file_extensions.iter().any(|e| e == extension);
            if !(is_module || extension == "snap")
                || haste_ignored.iter().any(|r| r.is_match(&text))
            {
                return;
            }
            files.push(path.to_path_buf());
            if !is_module {
                return;
            }
            if let Some((_, inside)) = text.split_once("/__mocks__/") {
                let name = inside
                    .strip_suffix(&format!(".{extension}"))
                    .unwrap_or(inside);
                manual_mocks.insert(name.to_string(), path.to_path_buf());
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
    files.sort();
    files.dedup();
    Ok(Found {
        tests: found,
        manual_mocks,
        files,
    })
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

/// Everything under `dir` but version-control folders and `node_modules`,
/// which jest-haste-map leaves out (`retainAllFiles: false`).
fn walk(dir: &Path, visit: &mut dyn FnMut(&Path)) {
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
            if name == ".git" || name == ".hg" || name == ".svn" || name == "node_modules" {
                continue;
            }
            walk(&path, visit);
        } else if kind.is_file() {
            visit(&path);
        }
    }
}
