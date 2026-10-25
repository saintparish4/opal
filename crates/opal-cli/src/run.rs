//! `opal run` and `opal x`: the halves of running code that need no
//! JavaScript engine. The npm semantics they follow are documented in
//! `opal_runtime::scripts` and `opal_runtime::bins`.

use std::io::IsTerminal;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use clap::Args;
use opal_pm::edit::AddRequest;
use opal_pm::install::{self, Change, InstallOptions};
use opal_runtime::bins;
use opal_runtime::scripts::{self, Context, Package, ScriptError, Step};
use opal_runtime::{signals, trace};

use crate::style::Paint;
use crate::{Failure, ProjectArgs, install_tools};

#[derive(Args)]
pub struct RunArgs {
    /// Succeed without doing anything when the script doesn't exist.
    #[arg(long)]
    if_present: bool,
    /// The script to run, then its arguments, which are appended to its
    /// command. With none, lists the project's scripts. Everything after the
    /// script's name is the script's, flags included; a `--` right after it
    /// is dropped, so `opal run test -- --watch` works as it does with npm.
    #[arg(
        trailing_var_arg = true,
        allow_hyphen_values = true,
        value_name = "SCRIPT [ARGS]"
    )]
    command: Vec<String>,
}

#[derive(Args)]
pub struct XArgs {
    /// Cache location. Defaults to $OPAL_CACHE_DIR, else the platform cache directory.
    #[arg(long)]
    cache_dir: Option<PathBuf>,
    /// Registry base URL. Defaults to $OPAL_REGISTRY, else the public registry.
    #[arg(long)]
    registry: Option<String>,
    /// Install from cached registry metadata only; never reach the network.
    #[arg(long)]
    offline: bool,
    /// The command or package to run: `tsc`, `cowsay`, `cowsay@1.6.0`,
    /// `@scope/tool@^2`.
    package: String,
    /// Arguments for the command, passed to it unchanged.
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    args: Vec<String>,
}

pub fn run_command(args: RunArgs) -> Result<ExitCode, Failure> {
    let cwd = std::env::current_dir()?;
    let project = scripts::find_project(&cwd).ok_or_else(|| ScriptError::NoProject(cwd.clone()))?;
    let package = Package::read(&project)?;
    let mut command = args.command.into_iter();
    let Some(name) = command.next() else {
        list(&package);
        return Ok(ExitCode::SUCCESS);
    };
    let mut script_args: Vec<String> = command.collect();
    if script_args.first().is_some_and(|arg| arg == "--") {
        script_args.remove(0);
    }
    let steps = match scripts::plan(&package, &name, &script_args) {
        Ok(steps) => steps,
        Err(ScriptError::Missing { .. }) if args.if_present => return Ok(ExitCode::SUCCESS),
        Err(error) => return Err(error.into()),
    };
    trace::checkpoint("script planned");
    let context = context(project, cwd, "run", Vec::new());
    let paint = Paint::for_stderr();
    let announce = |step: &Step| eprintln!("{}", paint.dim(format!("$ {}", step.command)));
    let outcome = scripts::run(&steps, &package, &context, &announce)?;
    signals::leave(outcome)
}

fn list(package: &Package) {
    if package.scripts.is_empty() {
        return;
    }
    let id = package.id();
    match id.is_empty() {
        true => println!("Scripts:"),
        false => println!("Scripts in {id}:"),
    }
    for (name, command) in &package.scripts {
        println!("  {name}\n    {command}");
    }
}

pub fn x_command(args: XArgs) -> Result<ExitCode, Failure> {
    let cwd = std::env::current_dir()?;
    let here = Package::read(&cwd)?;
    let (name, version) = bins::split_spec(&args.package);
    let paint = Paint::for_stderr();

    // A bare command is looked for where it already is before anything is
    // installed: the current package's own bin, then node_modules/.bin up
    // the tree.
    let (program, extra_bins) = match version {
        None => match own_bin(&cwd, name).or_else(|| bins::find_local_bin(&cwd, name)) {
            Some(program) => (program, Vec::new()),
            None => installed(&args, paint)?,
        },
        Some(_) => installed(&args, paint)?,
    };
    trace::checkpoint("command found");

    let mut script = format!("'{}'", name.replace('\'', "'\\''"));
    for arg in &args.args {
        script.push(' ');
        script.push_str(&scripts::sh_quote(arg));
    }
    let context = context(cwd.clone(), cwd, "exec", extra_bins);
    let env = scripts::environment(std::env::vars_os(), &here, &context, "npx", &script);
    trace::checkpoint("exec");
    trace::finish();
    let error = Command::new(&program)
        .args(&args.args)
        .env_clear()
        .envs(env)
        .exec();
    Err(format!("could not run {}: {error}", program.display()).into())
}

/// The current package's own `bin` named `command`, which npx runs before
/// looking anywhere else.
fn own_bin(cwd: &Path, command: &str) -> Option<PathBuf> {
    let text = std::fs::read_to_string(cwd.join("package.json")).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    let name = value.get("name").and_then(|n| n.as_str());
    let bin = match value.get("bin")? {
        serde_json::Value::String(path) if name?.rsplit('/').next() == Some(command) => {
            path.clone()
        }
        serde_json::Value::Object(map) => map.get(command)?.as_str()?.to_string(),
        _ => return None,
    };
    Some(cwd.join(bin))
}

/// The package installed into its own folder under the cache (installing it
/// the first time), and the command in it to run. A package is installed
/// once per spelling of its spec: `cowsay` and `cowsay@latest` are two
/// installs, and a tag or range is resolved when first installed, not again.
fn installed(args: &XArgs, paint: Paint) -> Result<(PathBuf, Vec<PathBuf>), Failure> {
    let project = ProjectArgs {
        root: None,
        cache_dir: args.cache_dir.clone(),
        registry: args.registry.clone(),
        production: false,
        offline: args.offline,
        prefer_offline: false,
    };
    let (cache, store, projects, registry) = install_tools(&project)?;
    let key = blake3::hash(args.package.as_bytes()).to_hex();
    let dir = cache.path().join("x").join(&key[..16]);
    let (name, _) = bins::split_spec(&args.package);
    let done = dir.join(".opal-x-installed");
    if !done.exists() {
        std::fs::create_dir_all(&dir)?;
        std::fs::write(dir.join("package.json"), "{ \"private\": true }\n")?;
        eprintln!("{}", paint.dim(format!("installing {}", args.package)));
        let change = Change::Add {
            requests: vec![AddRequest::parse(&args.package)?],
            group: None,
            exact: false,
        };
        install::change(
            &dir,
            &change,
            &registry,
            &store,
            &projects,
            &InstallOptions::default(),
            crate::progress::reporter().as_ref(),
        )?;
        std::fs::write(&done, "")?;
    }
    let package_dir = dir.join("node_modules").join(name);
    let manifest = opal_pm::manifest::Manifest::read(&package_dir.join("package.json"))?;
    let real_name = manifest.name.as_deref().unwrap_or(name);
    let command = bins::command_for(real_name, &manifest.bin)
        .ok_or_else(|| format!("could not tell which command of {real_name} to run"))?;
    let bin_dir = dir.join("node_modules").join(".bin");
    Ok((bin_dir.join(command), vec![bin_dir]))
}

fn context(
    project: PathBuf,
    init_cwd: PathBuf,
    command: &'static str,
    extra_bins: Vec<PathBuf>,
) -> Context {
    Context {
        project,
        init_cwd,
        opal: std::env::current_exe().unwrap_or_else(|_| PathBuf::from("opal")),
        opal_version: env!("CARGO_PKG_VERSION").to_string(),
        command,
        extra_bins,
        color: std::io::stdout().is_terminal(),
    }
}
