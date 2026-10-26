//! `opal run <script>`: a package.json script, run the way npm runs it.
//!
//! The reference is npm 12's own source, not its documentation:
//! `lib/commands/run.js` for which events run, and `@npmcli/run-script` for
//! how each one is started. What follows from them:
//!
//! - The project is the nearest directory, from the current one up, holding a
//!   `package.json` or a `node_modules` (`@npmcli/config`'s local prefix).
//!   Scripts run there, not in the directory opal was started from, which is
//!   kept in `INIT_CWD`.
//! - `pre<name>` and `post<name>` run around `<name>` when they are set and
//!   not empty. Arguments go to `<name>` alone, each quoted for `sh` the way
//!   `@npmcli/promise-spawn` quotes it, and appended to the command.
//! - A script is `sh -c <command>`, with every `node_modules/.bin` from the
//!   project up to `/` put in front of `PATH`, nearest first.
//! - The script's environment is opal's, plus `npm_package_*` for the
//!   package's name, version, config, engines, and bin (flattened as npm's
//!   `package-envs.js` does), `npm_package_json`, `npm_lifecycle_event`,
//!   `npm_lifecycle_script`, `npm_command`, `npm_config_local_prefix`,
//!   `npm_config_user_agent`, `npm_execpath`, `INIT_CWD`, `COLOR`, and
//!   `NODE`/`npm_node_execpath` when there is a `node` on `PATH`.
//! - The first script to fail stops the rest, and opal exits with its code;
//!   one killed by a signal kills opal with the same signal, so the shell
//!   that started opal sees what it would have seen without it.
//!
//! Where opal differs, on purpose:
//!
//! - With no `pre`/`post` script, opal replaces itself with the shell
//!   (`exec`) instead of starting it as a child: exit codes and signals then
//!   need no forwarding, and no process is left waiting.
//! - The `npm_config_*` variables npm exports for its own settings (cache,
//!   prefix, userconfig, node_gyp, ...) are not set: they describe npm, and
//!   opal has no such settings. Neither is npm's `node-gyp-bin` folder put on
//!   `PATH`, nor `EDITOR` set.
//! - The shell is `/bin/sh`; npm's `script-shell` setting and workspaces are
//!   not read.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::io;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

use crate::signals;
use crate::trace;

#[derive(Debug, thiserror::Error)]
pub enum ScriptError {
    #[error("no package.json in {0} or any folder above it")]
    NoProject(PathBuf),
    #[error("could not read {path}: {source}")]
    Read { path: PathBuf, source: io::Error },
    #[error("{path} is not valid JSON: {source}")]
    Parse {
        path: PathBuf,
        source: serde_json::Error,
    },
    #[error("missing script: \"{name}\"{available}")]
    Missing { name: String, available: String },
    #[error("could not start {program}: {source}")]
    Spawn { program: String, source: io::Error },
}

/// The nearest directory from `start` up that has a `package.json` or a
/// `node_modules` folder: npm's local prefix.
pub fn find_project(start: &Path) -> Option<PathBuf> {
    start
        .ancestors()
        .find(|dir| dir.join("package.json").is_file() || dir.join("node_modules").is_dir())
        .map(Path::to_path_buf)
}

/// A package.json as far as running its scripts needs it.
#[derive(Debug, Clone, Default)]
pub struct Package {
    pub name: Option<String>,
    pub version: Option<String>,
    /// In the order package.json lists them. Only string values count: npm
    /// drops the rest when it normalizes the file.
    pub scripts: Vec<(String, String)>,
    /// `npm_package_*`, flattened.
    pub env: Vec<(String, String)>,
}

impl Package {
    /// The package.json in `dir`. A directory with only `node_modules` is a
    /// project with no package.json, which npm reads as an empty one.
    pub fn read(dir: &Path) -> Result<Self, ScriptError> {
        let path = dir.join("package.json");
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(source) => return Err(ScriptError::Read { path, source }),
        };
        let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
        let value: Value =
            serde_json::from_str(text).map_err(|source| ScriptError::Parse { path, source })?;
        Ok(Self::from_value(&value))
    }

    pub fn from_value(value: &Value) -> Self {
        let name = value
            .get("name")
            .and_then(Value::as_str)
            .map(str::to_string);
        // As written: `npm run` doesn't clean it (that is a step of npm's
        // stricter normalization, which installs use), checked against npm
        // by tests/npm-run-cross-check.rs.
        let version = value
            .get("version")
            .and_then(Value::as_str)
            .map(str::to_string);
        let scripts = match value.get("scripts") {
            Some(Value::Object(scripts)) => scripts
                .iter()
                .filter_map(|(k, v)| v.as_str().map(|v| (k.clone(), v.to_string())))
                .collect(),
            _ => Vec::new(),
        };

        let mut fields = serde_json::Map::new();
        if let Some(name) = &name {
            fields.insert("name".into(), Value::String(name.clone()));
        }
        if let Some(version) = &version {
            fields.insert("version".into(), Value::String(version.clone()));
        }
        for key in ["config", "engines"] {
            if let Some(field) = value.get(key) {
                fields.insert(key.into(), field.clone());
            }
        }
        if let Some(bin) = normalize_bin(value.get("bin"), name.as_deref()) {
            fields.insert(
                "bin".into(),
                Value::Object(
                    bin.into_iter()
                        .map(|(k, v)| (k, Value::String(v)))
                        .collect(),
                ),
            );
        }
        let mut env = Vec::new();
        flatten("npm_package_", &fields, &mut env);
        Package {
            name,
            version,
            scripts,
            env,
        }
    }

    pub fn script(&self, name: &str) -> Option<&str> {
        self.scripts
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    /// `name@version`, or whatever of it the package has.
    pub fn id(&self) -> String {
        match (&self.name, self.version.as_deref()) {
            (Some(name), Some(version)) if !version.is_empty() => format!("{name}@{version}"),
            (Some(name), _) => name.clone(),
            _ => String::new(),
        }
    }
}

/// npm's `normalizePackageBin`: a string names one command after the package
/// (its scope dropped), an array names each command after its file, and every
/// key and path is made relative and kept inside the package.
fn normalize_bin(bin: Option<&Value>, name: Option<&str>) -> Option<BTreeMap<String, String>> {
    let entries: Vec<(String, String)> = match bin? {
        Value::String(path) => vec![(name?.to_string(), path.clone())],
        Value::Array(paths) => paths
            .iter()
            .filter_map(Value::as_str)
            .map(|p| (basename(p).to_string(), p.to_string()))
            .collect(),
        Value::Object(map) => map
            .iter()
            .filter_map(|(k, v)| v.as_str().map(|v| (k.clone(), v.to_string())))
            .collect(),
        _ => return None,
    };
    let mut out = BTreeMap::new();
    for (key, path) in entries {
        let key = basename(&secure_path(&key)).to_string();
        let path = secure_path(&path);
        if !key.is_empty() && !path.is_empty() {
            out.insert(key, path);
        }
    }
    (!out.is_empty()).then_some(out)
}

/// npm's `secureAndUnixifyPath`: backslashes become slashes, and the path is
/// resolved against `/` and made relative again, so `..` cannot leave the
/// package. A path that resolves to the package itself is empty.
fn secure_path(path: &str) -> String {
    let path = path.replace('\\', "/");
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            part => parts.push(part),
        }
    }
    let mut out = parts.join("/");
    if !out.is_empty() && path.ends_with('/') {
        out.push('/');
    }
    out
}

fn basename(path: &str) -> &str {
    path.trim_end_matches('/').rsplit('/').next().unwrap_or("")
}

/// npm's `package-envs.js`: nested objects join their keys with `_`, arrays
/// number their items, `null` and `false` become empty strings.
fn flatten(prefix: &str, fields: &serde_json::Map<String, Value>, out: &mut Vec<(String, String)>) {
    for (key, value) in fields {
        flatten_value(&format!("{prefix}{key}"), value, out);
    }
}

fn flatten_value(name: &str, value: &Value, out: &mut Vec<(String, String)>) {
    match value {
        Value::Null | Value::Bool(false) => out.push((name.to_string(), String::new())),
        Value::Bool(true) => out.push((name.to_string(), "true".into())),
        Value::Number(number) => out.push((name.to_string(), js_number(number))),
        Value::String(text) => out.push((name.to_string(), text.clone())),
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                flatten_value(&format!("{name}_{index}"), item, out);
            }
        }
        Value::Object(map) => flatten(&format!("{name}_"), map, out),
    }
}

/// `String(n)` for the numbers JSON holds: integers without a fraction.
fn js_number(number: &serde_json::Number) -> String {
    match number.as_f64() {
        Some(float) if float.fract() == 0.0 && float.abs() < 1e21 => format!("{float:.0}"),
        _ => number.to_string(),
    }
}

/// `@npmcli/promise-spawn`'s `escape.sh`: an argument with nothing the shell
/// would read specially is passed as it is; anything else is single-quoted.
pub fn sh_quote(argument: &str) -> String {
    if argument.is_empty() {
        return "''".into();
    }
    const SPECIAL: &[char] = &[
        '\t', '\n', '\r', ' ', '"', '#', '$', '&', '\'', '(', ')', '*', ';', '<', '>', '?', '\\',
        '`', '|', '~',
    ];
    if !argument.contains(SPECIAL) {
        return argument.to_string();
    }
    let quoted = format!("'{}'", argument.replace('\'', "'\\''"));
    // The same two clean-ups npm makes: no leading run of empty '' pairs, and
    // no '' left between an escaped quote and the closing one.
    let mut quoted = quoted.as_str();
    while quoted.starts_with("''") && quoted.len() > 2 {
        quoted = &quoted[2..];
    }
    quoted.replace("\\'''", "\\'")
}

/// One script to run: the event name and the command line handed to `sh -c`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub event: String,
    /// The script as package.json has it: `npm_lifecycle_script`.
    pub script: String,
    /// The script with this run's arguments appended.
    pub command: String,
}

/// The scripts `opal run <name> <args>` runs, in order.
pub fn plan(package: &Package, name: &str, args: &[String]) -> Result<Vec<Step>, ScriptError> {
    let main = match package.script(name) {
        Some(script) => script.to_string(),
        // npm's two built-in defaults.
        None if name == "env" => "env".to_string(),
        None if name == "restart" => "opal run stop --if-present && opal run start".to_string(),
        None => {
            let names: Vec<&str> = package.scripts.iter().map(|(k, _)| k.as_str()).collect();
            let available = match names.is_empty() {
                true => String::new(),
                false => format!(" (scripts: {})", names.join(", ")),
            };
            return Err(ScriptError::Missing {
                name: name.to_string(),
                available,
            });
        }
    };
    let mut steps = Vec::new();
    let mut push = |event: String, script: &str, args: &[String]| {
        // An empty script is set but does nothing; npm skips it.
        if script.is_empty() {
            return;
        }
        let mut command = script.to_string();
        for arg in args {
            command.push(' ');
            command.push_str(&sh_quote(arg));
        }
        steps.push(Step {
            event,
            script: script.to_string(),
            command,
        });
    };
    if let Some(pre) = package.script(&format!("pre{name}")) {
        push(format!("pre{name}"), pre, &[]);
    }
    push(name.to_string(), &main, args);
    if let Some(post) = package.script(&format!("post{name}")) {
        push(format!("post{name}"), post, &[]);
    }
    Ok(steps)
}

/// What a script's environment is built from, beyond opal's own.
#[derive(Debug, Clone)]
pub struct Context {
    /// The project directory: where scripts run, and where `.bin` lookup starts.
    pub project: PathBuf,
    /// Where opal was started: `INIT_CWD`.
    pub init_cwd: PathBuf,
    /// This opal binary: `npm_execpath`, so tools that re-invoke "the package
    /// manager" (npm-run-all, for one) invoke opal.
    pub opal: PathBuf,
    pub opal_version: String,
    /// `npm_command`: `run` for scripts, `exec` for `opal x`.
    pub command: &'static str,
    /// `.bin` folders to put before the project's (an `opal x` install).
    pub extra_bins: Vec<PathBuf>,
    pub color: bool,
}

/// The environment for one script: `base` (opal's own environment) with the
/// npm variables set on top.
pub fn environment(
    base: impl IntoIterator<Item = (OsString, OsString)>,
    package: &Package,
    context: &Context,
    event: &str,
    script: &str,
) -> Vec<(OsString, OsString)> {
    let mut env: BTreeMap<OsString, OsString> = base.into_iter().collect();
    let node = env
        .get(OsStr::new("PATH"))
        .and_then(|path| find_in_path(path, "node"));
    let mut set = |key: &str, value: &OsStr| {
        env.insert(key.into(), value.to_os_string());
    };
    for (key, value) in &package.env {
        set(key, OsStr::new(value));
    }
    set(
        "npm_package_json",
        context.project.join("package.json").as_os_str(),
    );
    set("npm_lifecycle_event", OsStr::new(event));
    set("npm_lifecycle_script", OsStr::new(script));
    set("npm_command", OsStr::new(context.command));
    set("npm_config_local_prefix", context.project.as_os_str());
    let agent = format!(
        "opal/{} {} {}",
        context.opal_version,
        std::env::consts::OS,
        match std::env::consts::ARCH {
            "x86_64" => "x64",
            "aarch64" => "arm64",
            other => other,
        }
    );
    set("npm_config_user_agent", OsStr::new(&agent));
    set("npm_execpath", context.opal.as_os_str());
    set("INIT_CWD", context.init_cwd.as_os_str());
    set("COLOR", OsStr::new(if context.color { "1" } else { "0" }));
    if let Some(node) = node {
        set("NODE", node.as_os_str());
        set("npm_node_execpath", node.as_os_str());
    }
    // Only an existing PATH is rewritten, as npm's set-path.js does.
    if let Some(path) = env.get(OsStr::new("PATH")) {
        let mut entries: Vec<PathBuf> = context.extra_bins.clone();
        entries.extend(
            context
                .project
                .ancestors()
                .map(|dir| dir.join("node_modules").join(".bin")),
        );
        let mut joined = std::env::join_paths(entries).unwrap_or_default();
        joined.push(":");
        joined.push(path);
        env.insert("PATH".into(), joined);
    }
    env.into_iter().collect()
}

fn find_in_path(path: &OsStr, program: &str) -> Option<PathBuf> {
    std::env::split_paths(path)
        .map(|dir| dir.join(program))
        .find(|candidate| is_executable(candidate))
}

pub(crate) fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

/// How a run ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Exited(i32),
    Signaled(i32),
}

const SHELL: &str = "/bin/sh";

/// Runs `steps`, printing each command through `announce` first. With one
/// step, the process becomes the shell and this returns only on failure to
/// start it.
pub fn run(
    steps: &[Step],
    package: &Package,
    context: &Context,
    announce: &dyn Fn(&Step),
) -> Result<Outcome, ScriptError> {
    let base: Vec<(OsString, OsString)> = std::env::vars_os().collect();
    let command = |step: &Step| {
        let mut command = Command::new(SHELL);
        command
            .arg("-c")
            .arg(&step.command)
            .current_dir(&context.project)
            .env_clear()
            .envs(environment(
                base.iter().cloned(),
                package,
                context,
                &step.event,
                &step.script,
            ));
        command
    };
    if let [step] = steps {
        announce(step);
        trace::checkpoint("exec");
        trace::finish();
        let source = command(step).arg0("sh").exec();
        return Err(ScriptError::Spawn {
            program: SHELL.into(),
            source,
        });
    }
    for step in steps {
        announce(step);
        let outcome =
            signals::run_forwarding(&mut command(step)).map_err(|source| ScriptError::Spawn {
                program: SHELL.into(),
                source,
            })?;
        match outcome {
            Outcome::Exited(0) => {}
            failed => return Ok(failed),
        }
    }
    Ok(Outcome::Exited(0))
}

/// The exit status a child had, as an [`Outcome`].
pub(crate) fn outcome(status: std::process::ExitStatus) -> Outcome {
    match (status.code(), status.signal()) {
        (Some(code), _) => Outcome::Exited(code),
        (None, Some(signal)) => Outcome::Signaled(signal),
        (None, None) => Outcome::Exited(1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn package(value: Value) -> Package {
        Package::from_value(&value)
    }

    fn env_of(package: &Package) -> BTreeMap<String, String> {
        package.env.iter().cloned().collect()
    }

    #[test]
    fn package_env_flattens_the_fields_npm_exports() {
        let package = package(json!({
            "name": "app", "version": "v1.2.3",
            "config": { "port": 8080, "nested": { "a": [1, "two", null] }, "off": false },
            "engines": { "node": ">=18" },
            "bin": "./cli.js",
            "description": "not exported"
        }));
        let env = env_of(&package);
        assert_eq!(env["npm_package_name"], "app");
        assert_eq!(env["npm_package_version"], "v1.2.3");
        assert_eq!(env["npm_package_config_port"], "8080");
        assert_eq!(env["npm_package_config_nested_a_0"], "1");
        assert_eq!(env["npm_package_config_nested_a_1"], "two");
        assert_eq!(env["npm_package_config_nested_a_2"], "");
        assert_eq!(env["npm_package_config_off"], "");
        assert_eq!(env["npm_package_engines_node"], ">=18");
        assert_eq!(env["npm_package_bin_app"], "cli.js");
        assert!(!env.contains_key("npm_package_description"));
    }

    #[test]
    fn a_missing_name_or_version_is_not_exported() {
        let env = env_of(&package(json!({})));
        assert!(!env.contains_key("npm_package_version"));
        assert!(!env.contains_key("npm_package_name"));
    }

    #[test]
    fn bins_are_normalized_like_npm() {
        let bin = |value: Value, name: Option<&str>| normalize_bin(Some(&value), name);
        assert_eq!(
            bin(json!("bin/x.js"), Some("@scope/tool")).unwrap(),
            BTreeMap::from([("tool".into(), "bin/x.js".into())])
        );
        assert_eq!(
            bin(json!({"../../evil": "../../../etc/passwd", "ok": "./a\\b.js", "bad": 1, "empty": "."}), None)
                .unwrap(),
            BTreeMap::from([
                ("evil".into(), "etc/passwd".into()),
                ("ok".into(), "a/b.js".into())
            ])
        );
        assert_eq!(
            bin(json!(["bin/one", "two"]), None).unwrap(),
            BTreeMap::from([
                ("one".into(), "bin/one".into()),
                ("two".into(), "two".into())
            ])
        );
        assert_eq!(bin(json!({}), None), None);
    }

    #[test]
    fn non_string_scripts_are_dropped() {
        let package = package(json!({"scripts": {"a": "echo a", "b": 1, "c": null}}));
        assert_eq!(
            package.scripts,
            vec![("a".to_string(), "echo a".to_string())]
        );
    }

    #[test]
    fn sh_quote_matches_npm_escape_sh() {
        // Expected values from @npmcli/promise-spawn's escape.sh.
        assert_eq!(sh_quote(""), "''");
        assert_eq!(sh_quote("plain"), "plain");
        assert_eq!(sh_quote("--flag=value"), "--flag=value");
        assert_eq!(sh_quote("two words"), "'two words'");
        assert_eq!(sh_quote("$HOME"), "'$HOME'");
        assert_eq!(sh_quote("it's"), "'it'\\''s'");
        assert_eq!(sh_quote("'quoted'"), "\\''quoted'\\'");
    }

    #[test]
    fn plan_wraps_the_script_in_pre_and_post_and_gives_arguments_to_it_alone() {
        let package = package(json!({"scripts": {
            "pretest": "echo pre", "test": "jest", "posttest": "echo post"
        }}));
        let steps = plan(&package, "test", &["--watch".into(), "a b".into()]).unwrap();
        let commands: Vec<_> = steps.iter().map(|s| s.command.as_str()).collect();
        assert_eq!(commands, ["echo pre", "jest --watch 'a b'", "echo post"]);
        assert_eq!(steps[1].script, "jest");
        assert_eq!(steps[0].event, "pretest");
    }

    #[test]
    fn empty_scripts_are_skipped_and_a_missing_one_is_an_error() {
        let package = package(json!({"scripts": {"prebuild": "", "build": "tsc"}}));
        assert_eq!(plan(&package, "build", &[]).unwrap().len(), 1);
        let package = package_with_empty_main();
        assert!(plan(&package, "noop", &[]).unwrap().is_empty());
        assert!(matches!(
            plan(&package, "absent", &[]),
            Err(ScriptError::Missing { .. })
        ));
    }

    fn package_with_empty_main() -> Package {
        package(json!({"scripts": {"noop": ""}}))
    }

    #[test]
    fn env_is_a_built_in_script() {
        let steps = plan(&Package::default(), "env", &[]).unwrap();
        assert_eq!(steps[0].command, "env");
    }

    #[test]
    fn path_gets_every_ancestor_bin_folder_nearest_first() {
        let context = Context {
            project: PathBuf::from("/work/app"),
            init_cwd: PathBuf::from("/work/app/src"),
            opal: PathBuf::from("/opt/opal"),
            opal_version: "0.0.0".into(),
            command: "run",
            extra_bins: vec![],
            color: false,
        };
        let env: BTreeMap<_, _> = environment(
            [("PATH".into(), "/usr/bin".into())],
            &Package::default(),
            &context,
            "test",
            "jest",
        )
        .into_iter()
        .collect();
        assert_eq!(
            env[OsStr::new("PATH")],
            "/work/app/node_modules/.bin:/work/node_modules/.bin:/node_modules/.bin:/usr/bin"
        );
        assert_eq!(env[OsStr::new("INIT_CWD")], "/work/app/src");
        assert_eq!(env[OsStr::new("npm_lifecycle_event")], "test");
        assert_eq!(
            env[OsStr::new("npm_package_json")],
            "/work/app/package.json"
        );
    }
}
