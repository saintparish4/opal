//! The project configuration, as the runtime's `config.cjs` normalized it.
//!
//! Jest configs are JavaScript, so a worker evaluates them and sends back
//! one JSON object with every default filled in and `<rootDir>` expanded.
//! opal reads the fields it needs to find test files, and passes the whole
//! object back to every worker as it was.

use std::path::PathBuf;

use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectConfig {
    pub root_dir: PathBuf,
    pub roots: Vec<PathBuf>,
    #[serde(default)]
    pub test_match: Vec<String>,
    #[serde(default)]
    pub test_regex: Vec<String>,
    #[serde(default)]
    pub test_path_ignore_patterns: Vec<String>,
    pub module_file_extensions: Vec<String>,
    /// Where the config came from, for messages; `None` for the defaults.
    #[serde(default)]
    pub config_path: Option<PathBuf>,
}

impl ProjectConfig {
    pub fn from_value(value: &Value) -> Result<Self, serde_json::Error> {
        ProjectConfig::deserialize(value)
    }
}
