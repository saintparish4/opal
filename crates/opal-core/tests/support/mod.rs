//! Fixtures shared by the integration suites.
//!
//! Projects are built in temp directories rather than checked in as trees,
//! because half these tests need to mutate a project between runs — and a test
//! that edits a checked-in fixture is a test that fails differently the second
//! time you run it.

#![allow(dead_code)]

use std::fs;
use std::path::PathBuf;

use opal_core::cache::CacheRoot;
use opal_core::graph::GraphCache;
use opal_core::path::NormalizedPath;
use tempfile::TempDir;

pub struct Project {
    directory: TempDir,
}

impl Project {
    pub fn new() -> Self {
        Self {
            directory: tempfile::tempdir().expect("temp dir"),
        }
    }

    pub fn root(&self) -> NormalizedPath {
        NormalizedPath::from_native(self.directory.path()).expect("utf-8 temp path")
    }

    pub fn native(&self, relative: &str) -> PathBuf {
        self.directory.path().join(relative)
    }

    pub fn write(&self, relative: &str, contents: &str) -> &Self {
        let path = self.native(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create parent");
        }
        fs::write(&path, contents).expect("write fixture");
        self
    }

    pub fn remove(&self, relative: &str) -> &Self {
        fs::remove_file(self.native(relative)).expect("remove fixture");
        self
    }

    /// Changes only the modification time, leaving bytes untouched.
    pub fn touch_mtime(&self, relative: &str) -> &Self {
        use std::time::{Duration, SystemTime};

        let path = self.native(relative);
        let times = fs::FileTimes::new()
            .set_modified(SystemTime::now() + Duration::from_secs(600))
            .set_accessed(SystemTime::now() + Duration::from_secs(600));
        fs::File::options()
            .write(true)
            .open(&path)
            .expect("open fixture")
            .set_times(times)
            .expect("set times");
        self
    }
}

impl Default for Project {
    fn default() -> Self {
        Self::new()
    }
}

/// A cache directory plus the handle onto it. The `TempDir` must stay alive for
/// as long as the cache is used, so it is returned rather than dropped.
pub fn cache() -> (TempDir, GraphCache) {
    let directory = tempfile::tempdir().expect("temp dir");
    let cache = CacheRoot::at(directory.path()).open().expect("open cache");
    (directory, cache)
}

/// Opens a second handle onto an existing cache — what a second tool does.
pub fn reopen(directory: &TempDir) -> GraphCache {
    CacheRoot::at(directory.path()).open().expect("open cache")
}

pub fn entry(path: &str) -> NormalizedPath {
    NormalizedPath::new(path)
}

/// A project shaped like a real one: a scoped dependency with an `exports` map,
/// a cycle, a JSON import, a builtin, a TypeScript file reached through its
/// emitted `.js` name, a type-only import, a dynamic import, and a dependency
/// that is not installed.
pub fn realistic_app() -> Project {
    let project = Project::new();
    project
        .write(
            "package.json",
            r#"{ "name": "app", "version": "1.0.0", "type": "module" }"#,
        )
        .write(
            "src/index.js",
            r#"import { util } from './util.js';
import path from 'node:path';
import widget from '@scope/widget';
import missing from 'not-installed';
import { typed } from './typed.js';
export * from './reexport.js';

export async function main() {
  const lazy = await import('./lazy.js');
  return [util, path, widget, missing, typed, lazy];
}
"#,
        )
        .write(
            "src/util.js",
            r#"import data from './data.json' with { type: 'json' };
import { help } from './helper';

export const util = { data, help };
"#,
        )
        .write(
            "src/helper.js",
            r#"import { util } from './util.js';

export const help = () => util;
"#,
        )
        .write("src/data.json", "{ \"ok\": true }\n")
        .write("src/lazy.js", "export const lazy = 1;\n")
        .write("src/reexport.js", "export const reexported = 1;\n")
        .write(
            "src/typed.ts",
            r#"import type { Shape } from './types';
import { help } from './helper.js';

export const typed: Shape = { help };
"#,
        )
        .write("src/types.ts", "export interface Shape { help: unknown }\n")
        .write(
            "node_modules/@scope/widget/package.json",
            r#"{
  "name": "@scope/widget",
  "version": "2.0.0",
  "exports": {
    ".": { "import": "./esm/index.mjs", "require": "./cjs/index.cjs" },
    "./deep/*": "./src/*.js"
  }
}"#,
        )
        .write(
            "node_modules/@scope/widget/esm/index.mjs",
            "import { inner } from './inner.mjs';\nexport default inner;\n",
        )
        .write(
            "node_modules/@scope/widget/esm/inner.mjs",
            "export const inner = 'widget';\n",
        )
        .write(
            "node_modules/@scope/widget/cjs/index.cjs",
            "module.exports = require('./inner.cjs');\n",
        )
        .write(
            "node_modules/@scope/widget/cjs/inner.cjs",
            "module.exports = 'widget';\n",
        );
    project
}
