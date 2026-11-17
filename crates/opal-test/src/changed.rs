//! The files a working copy has changed, as jest-changed-files finds them with
//! git: what `--onlyChanged` and `--watch` start from.
//!
//! For each root, the repository holding it (`git rev-parse --show-cdup`),
//! then in each repository, limited to the roots, the staged files
//! (`git diff --cached --name-only`) and the modified and untracked ones
//! (`git ls-files --other --modified --exclude-standard`). Mercurial and
//! Sapling repositories, which Jest also reads, are not supported.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::TestError;

pub fn changed_files(roots: &[PathBuf]) -> Result<Vec<PathBuf>, TestError> {
    let mut repos: BTreeSet<PathBuf> = BTreeSet::new();
    for root in roots {
        if let Some(repo) = repository_of(root) {
            repos.insert(repo);
        }
    }
    if repos.is_empty() {
        return Err(TestError::Config(
            "--onlyChanged and --watch need a git repository to find what changed; use --watchAll to run every test"
                .to_string(),
        ));
    }
    let mut changed = BTreeSet::new();
    for repo in &repos {
        let include: Vec<String> = roots
            .iter()
            .filter_map(|root| root.strip_prefix(repo).ok())
            .map(|relative| {
                let text = relative.to_string_lossy().to_string();
                if text.is_empty() {
                    ".".to_string()
                } else {
                    text
                }
            })
            .collect();
        for args in [
            &["diff", "--cached", "--name-only"][..],
            &["ls-files", "--other", "--modified", "--exclude-standard"][..],
        ] {
            let output = Command::new("git")
                .args(args)
                .arg("--")
                .args(&include)
                .current_dir(repo)
                .output()
                .map_err(|error| TestError::Config(format!("could not run git: {error}")))?;
            for line in String::from_utf8_lossy(&output.stdout).lines() {
                if !line.is_empty() {
                    changed.insert(repo.join(line));
                }
            }
        }
    }
    Ok(changed.into_iter().collect())
}

fn repository_of(root: &Path) -> Option<PathBuf> {
    let output = Command::new("git")
        .args(["rev-parse", "--show-cdup"])
        .current_dir(root)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let up = String::from_utf8_lossy(&output.stdout).trim().to_string();
    // path.resolve's way: `..` taken off lexically, symlinks left alone, so
    // the roots stay prefixes of it.
    let mut repo = PathBuf::new();
    for component in root.join(up).components() {
        match component {
            std::path::Component::ParentDir => {
                repo.pop();
            }
            std::path::Component::CurDir => {}
            other => repo.push(other),
        }
    }
    Some(repo)
}
