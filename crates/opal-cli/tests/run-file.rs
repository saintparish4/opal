//! `opal run <file>`: running JavaScript on the embedded engine.
//!
//! Each expected output here was taken from Node 24.19.0 (the release the
//! runtime is measured against) running the same file, so these tests need no
//! node and run in the ordinary suite. `node-cross-check.rs` compares against
//! a live node instead, for the larger corpora.

use std::path::Path;
use std::process::{Command, Output, Stdio};

const OPAL: &str = env!("CARGO_BIN_EXE_opal");
const VERSION: &str = env!("CARGO_PKG_VERSION");

struct Run {
    stdout: String,
    stderr: String,
    code: Option<i32>,
}

fn run_in(dir: &Path, file: &str, args: &[&str]) -> Run {
    let output: Output = Command::new(OPAL)
        .current_dir(dir)
        .arg("run")
        .arg(file)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .expect("run opal");
    Run {
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        code: output.status.code(),
    }
}

/// Writes `source` to `main.js` in a fresh directory and runs it.
fn run(source: &str) -> (tempfile::TempDir, Run) {
    run_args(source, &[])
}

fn run_args(source: &str, args: &[&str]) -> (tempfile::TempDir, Run) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("main.js"), source).unwrap();
    let result = run_in(dir.path(), "main.js", args);
    (dir, result)
}

/// The path opal reports for main.js: the directory's real path, since the
/// main module is run by its real path, as in Node.
fn main_path(dir: &tempfile::TempDir) -> String {
    std::fs::canonicalize(dir.path().join("main.js"))
        .unwrap()
        .display()
        .to_string()
}

#[test]
fn hello_world() {
    let (_dir, out) = run("console.log(\"hello\")\n");
    assert_eq!(out.stdout, "hello\n");
    assert_eq!(out.stderr, "");
    assert_eq!(out.code, Some(0));
}

#[test]
fn argv_is_exec_path_then_the_main_module_then_the_arguments() {
    let (dir, out) = run_args(
        "console.log(JSON.stringify(process.argv.slice(1)))\n",
        &["a", "--flag", "-x"],
    );
    let expected = format!("[\"{}\",\"a\",\"--flag\",\"-x\"]\n", main_path(&dir));
    assert_eq!(out.stdout, expected);
}

#[test]
fn exit_codes_come_from_exit_code_and_exit() {
    let (_dir, out) = run("process.exitCode = 3\n");
    assert_eq!(out.code, Some(3));
    let (_dir, out) = run(
        "process.on('exit', (c) => console.log('exit', c));\nprocess.exit(4);\nconsole.log('not reached');\n",
    );
    assert_eq!(out.stdout, "exit 4\n");
    assert_eq!(out.code, Some(4));
}

#[test]
fn an_uncaught_error_is_reported_as_node_reports_it() {
    let (dir, out) = run("console.log(\"a\");\nfunction f() {\n  null.x;\n}\nf();\n");
    assert_eq!(out.stdout, "a\n");
    assert_eq!(out.code, Some(1));
    let path = main_path(&dir);
    let head = format!(
        "{path}:3\n  null.x;\n       ^\n\nTypeError: Cannot read properties of null (reading 'x')\n    at f ({path}:3:8)\n    at Object.<anonymous> ({path}:5:1)\n"
    );
    assert!(out.stderr.starts_with(&head), "{}", out.stderr);
    assert!(
        out.stderr.ends_with(&format!("\n\nOpal v{VERSION}\n")),
        "{}",
        out.stderr
    );
}

#[test]
fn a_thrown_value_that_is_not_an_error_gets_the_arrow_first() {
    let (dir, out) = run("throw {a: 1};\n");
    let path = main_path(&dir);
    assert_eq!(
        out.stderr,
        format!("\n{path}:1\nthrow {{a: 1}};\n^\n{{ a: 1 }}\n\nOpal v{VERSION}\n")
    );
    assert_eq!(out.code, Some(1));
}

#[test]
fn exit_listeners_run_before_the_report_with_code_1() {
    let (_dir, out) = run(
        "process.on('exit', (c) => console.log('exit', c, process.exitCode));\nthrow new Error('x');\n",
    );
    assert_eq!(out.stdout, "exit 1 1\n");
    assert_eq!(out.code, Some(1));
}

#[test]
fn an_unhandled_rejection_ends_the_process() {
    let (_dir, out) = run("Promise.reject(42);\nsetTimeout(() => console.log('never'), 10);\n");
    assert_eq!(out.stdout, "");
    assert_eq!(out.code, Some(1));
    assert!(
        out.stderr.contains(
            "UnhandledPromiseRejection: This error originated either by throwing inside of an async function without a catch block, or by rejecting a promise which was not handled with .catch(). The promise rejected with the reason \"42\"."
        ),
        "{}",
        out.stderr
    );
    assert!(out.stderr.contains("code: 'ERR_UNHANDLED_REJECTION'"));
}

#[test]
fn a_rejection_handled_late_in_the_same_tick_is_not_reported() {
    let (_dir, out) = run(
        "const p = Promise.reject(new Error('x'));\nqueueMicrotask(() => p.catch(() => console.log('caught')));\n",
    );
    assert_eq!(out.stdout, "caught\n");
    assert_eq!(out.code, Some(0));
}

#[test]
fn ticks_then_microtasks_then_timers_then_immediates() {
    let (_dir, out) = run(
        "setTimeout(()=>console.log(1));setImmediate(()=>console.log(2));process.nextTick(()=>console.log(3));Promise.resolve().then(()=>console.log(4));queueMicrotask(()=>console.log(5));\n",
    );
    assert_eq!(out.stdout, "3\n4\n5\n1\n2\n");
}

#[test]
fn ticks_and_microtasks_drain_between_timers() {
    let (_dir, out) = run(
        "setTimeout(() => { console.log('t1'); process.nextTick(() => console.log('tick')); Promise.resolve().then(() => console.log('micro')); });\nsetTimeout(() => console.log('t2'));\n",
    );
    assert_eq!(out.stdout, "t1\ntick\nmicro\nt2\n");
}

#[test]
fn intervals_repeat_until_cleared_and_unref_timers_do_not_keep_it_alive() {
    let (_dir, out) = run(
        "let n = 0;\nconst i = setInterval(() => { console.log('tick', ++n); if (n === 3) clearInterval(i); }, 1);\nsetTimeout(() => console.log('never'), 100000).unref();\n",
    );
    assert_eq!(out.stdout, "tick 1\ntick 2\ntick 3\n");
    assert_eq!(out.code, Some(0));
}

#[test]
fn before_exit_can_schedule_more_work() {
    let (_dir, out) = run(
        "process.on('beforeExit', c => { console.log('before', c); if (!globalThis.d) { globalThis.d=1; setTimeout(()=>console.log('t'),1) } });\n",
    );
    assert_eq!(out.stdout, "before 0\nt\nbefore 0\n");
}

#[test]
fn an_uncaught_exception_listener_keeps_the_loop_going() {
    let (_dir, out) = run(
        "process.on('uncaughtException', e => console.log('caught', e.message));\nsetTimeout(()=>console.log('later'),1);\nthrow new Error('x');\n",
    );
    assert_eq!(out.stdout, "caught x\nlater\n");
    assert_eq!(out.code, Some(0));
}

#[test]
fn bad_arguments_throw_nodes_errors() {
    let (_dir, out) = run(
        "for (const f of [() => setTimeout('x'), () => process.nextTick(1), () => { process.exitCode = 1.5 }, () => require(1)]) {\n  try { f() } catch (e) { console.log(e.code, e.message) }\n}\n",
    );
    assert_eq!(
        out.stdout,
        "ERR_INVALID_ARG_TYPE The \"callback\" argument must be of type function. Received type string ('x')\n\
         ERR_INVALID_ARG_TYPE The \"callback\" argument must be of type function. Received type number (1)\n\
         ERR_OUT_OF_RANGE The value of \"code\" is out of range. It must be an integer. Received 1.5\n\
         ERR_INVALID_ARG_TYPE The \"id\" argument must be of type string. Received type number (1)\n"
    );
}

#[test]
fn process_env_holds_strings() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("main.js"),
        "process.env.N = 1;\nconsole.log(typeof process.env.N, process.env.N, process.env.FROM_PARENT);\n",
    )
    .unwrap();
    let output = Command::new(OPAL)
        .current_dir(dir.path())
        .args(["run", "main.js"])
        .env("FROM_PARENT", "kept")
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&output.stdout), "string 1 kept\n");
}

#[test]
fn builtins_resolve_with_and_without_the_node_scheme() {
    let (dir, out) = run(
        "const path = require('path');\nconsole.log(path.join('/a', 'b', '../c'), require('node:events') === require('events'), typeof require('buffer').Buffer);\nrequire('nope');\n",
    );
    assert_eq!(out.stdout, "/a/c true function\n");
    let path = main_path(&dir);
    assert!(
        out.stderr.contains(&format!(
            "Error: Cannot find module 'nope'\nRequire stack:\n- {path}"
        )),
        "{}",
        out.stderr
    );
    assert!(out.stderr.contains("code: 'MODULE_NOT_FOUND'"));
}

#[test]
fn a_shebang_line_is_skipped() {
    let (_dir, out) = run(
        "#!/usr/bin/env node\nconsole.log(__filename.endsWith('main.js'), typeof module.exports)\n",
    );
    assert_eq!(out.stdout, "true object\n");
}

#[test]
fn a_closed_stdout_is_not_an_error() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("main.js"),
        "for (let i = 0; i < 100000; i++) console.log('line', i);\n",
    )
    .unwrap();
    let mut child = Command::new(OPAL)
        .current_dir(dir.path())
        .args(["run", "main.js"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // Read one line and close the pipe, as `| head -1` does.
    let stdout = child.stdout.take().unwrap();
    let mut first = String::new();
    std::io::BufRead::read_line(&mut std::io::BufReader::new(stdout), &mut first).unwrap();
    let output = child.wait_with_output().unwrap();
    assert_eq!(first, "line 0\n");
    assert_eq!(String::from_utf8_lossy(&output.stderr), "");
    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn an_es_module_runs_as_one() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("main.mjs"),
        "import { sep } from 'node:path';\nconsole.log(sep, typeof require, import.meta.main);\n",
    )
    .unwrap();
    let out = run_in(dir.path(), "main.mjs", &[]);
    assert_eq!(out.stdout, "/ undefined true\n", "{}", out.stderr);
    assert_eq!(out.code, Some(0));
}

#[test]
fn a_name_that_is_not_a_file_is_a_script() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("package.json"),
        r#"{ "scripts": { "build.js": "echo script" } }"#,
    )
    .unwrap();
    let out = run_in(dir.path(), "build.js", &[]);
    assert_eq!(out.stdout, "script\n");
}

#[test]
fn process_is_an_event_emitter_whenever_anyone_can_tell() {
    // The first listener makes it one...
    let (_dir, out) = run(
        "process.once('exit', (c) => console.log('exit', c));\nconst EventEmitter = require('events');\nconsole.log(process instanceof EventEmitter, process.listenerCount('exit'));\n",
    );
    assert_eq!(out.stdout, "true 1\nexit 0\n");
    // ...and so does loading events before any listener exists.
    let (_dir, out) = run(
        "const { EventEmitter } = require('node:events');\nconsole.log(process instanceof EventEmitter, process.eventNames().includes('warning'));\nprocess.stdout.on('x', () => console.log('stdout is one too'));\nprocess.stdout.emit('x');\n",
    );
    assert_eq!(out.stdout, "true true\nstdout is one too\n");
}

#[test]
fn warnings_print_as_node_prints_them() {
    let (_dir, out) = run("process.emitWarning('careful', 'CustomWarning', 'C001');\n");
    let pid_end = out.stderr.find(')').unwrap();
    assert!(out.stderr.starts_with("(node:"), "{}", out.stderr);
    assert_eq!(
        &out.stderr[pid_end..pid_end + 31],
        ") [C001] CustomWarning: careful"
    );
}

/// Writes `files` into a fresh directory and runs `main` with the cache in
/// that directory too, so a test can see what was cached.
fn run_files(files: &[(&str, &str)], main: &str) -> (tempfile::TempDir, Run) {
    let dir = tempfile::tempdir().unwrap();
    for (name, source) in files {
        let path = dir.path().join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, source).unwrap();
    }
    let output = Command::new(OPAL)
        .current_dir(dir.path())
        .args(["run", main])
        .env("OPAL_CACHE_DIR", dir.path().join("cache"))
        .output()
        .unwrap();
    let out = Run {
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        code: output.status.code(),
    };
    (dir, out)
}

#[test]
fn typescript_runs_with_its_types_stripped_and_its_columns_kept() {
    let source = "const x: number = 1;\ninterface I { a: string }\nfunction f<T>(a: T, b?: string): T { return a as T; }\nconsole.log(f(x), new Error('at').stack!.split('\\n')[1].trim());\n";
    let (dir, out) = run_files(&[("main.ts", source)], "main.ts");
    let url = format!(
        "file://{}",
        std::fs::canonicalize(dir.path().join("main.ts"))
            .unwrap()
            .display()
    );
    assert_eq!(
        out.stdout,
        format!("1 at Object.<anonymous> ({url}:4:19)\n"),
        "{}",
        out.stderr
    );
    // The output was cached: an index entry names its CAS object.
    let index = dir.path().join("cache/strip");
    assert_eq!(std::fs::read_dir(&index).unwrap().count(), 1);
    // A second run reads it back and prints the same.
    let output = Command::new(OPAL)
        .current_dir(dir.path())
        .args(["run", "main.ts"])
        .env("OPAL_CACHE_DIR", dir.path().join("cache"))
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&output.stdout), out.stdout);
}

#[test]
fn typescript_that_needs_a_transform_is_refused_as_node_refuses_it() {
    let (dir, out) = run_files(&[("main.ts", "let x = 1; enum E { A }\nnext\n")], "main.ts");
    let url = format!(
        "file://{}",
        std::fs::canonicalize(dir.path().join("main.ts"))
            .unwrap()
            .display()
    );
    assert!(
        out.stderr.starts_with(&format!(
            "{url}:1\nlet x = 1; enum E {{ A }}\n           ^^^^^^^^^^^^\nnext\n\nSyntaxError [ERR_UNSUPPORTED_TYPESCRIPT_SYNTAX]: TypeScript enum is not supported in strip-only mode\n"
        )),
        "{}",
        out.stderr
    );
    assert_eq!(out.code, Some(1));
}

#[test]
fn typescript_under_node_modules_is_not_stripped() {
    let (_dir, out) = run_files(
        &[("node_modules/p/main.ts", "let x: number = 1\n")],
        "node_modules/p/main.ts",
    );
    assert!(out.stderr.contains("Error [ERR_UNSUPPORTED_NODE_MODULES_TYPE_STRIPPING]: Stripping types is currently unsupported for files under node_modules"), "{}", out.stderr);
}

#[test]
fn a_typeless_js_or_ts_file_with_module_syntax_runs_as_an_es_module() {
    for name in ["main.ts", "main.js", "main.mts"] {
        let (_dir, out) = run_files(
            &[(
                name,
                "import { join } from 'node:path';\nconsole.log(join('a', 'b'))\n",
            )],
            name,
        );
        assert_eq!(out.stdout, "a/b\n", "{name}: {}", out.stderr);
    }
}

#[test]
fn fs_reads_and_writes_files_and_fails_as_node_fails() {
    let source = "const fs = require(\"fs\");\nfs.writeFileSync(\"out.txt\", \"written\");\nconsole.log(fs.readFileSync(\"out.txt\", \"utf8\"), fs.readdirSync(\".\"));\nfs.readFileSync(\"missing.txt\");\n";
    let (dir, out) = run(source);
    assert_eq!(out.stdout, "written [ 'main.js', 'out.txt' ]\n");
    assert_eq!(out.code, Some(1));
    let path = main_path(&dir);
    let head = format!(
        "node:fs:622\n  return binding.open(\n                 ^\n\nError: ENOENT: no such file or directory, open 'missing.txt'\n    at Object.openSync (node:fs:622:18)\n    at Object.readFileSync (node:fs:488:35)\n    at Object.<anonymous> ({path}:4:4)\n"
    );
    assert!(out.stderr.starts_with(&head), "{}", out.stderr);
    assert!(
        out.stderr.contains(
            "{\n  errno: -2,\n  code: 'ENOENT',\n  syscall: 'open',\n  path: 'missing.txt'\n}\n"
        ),
        "{}",
        out.stderr
    );
}

#[test]
fn fs_functions_with_a_callback_say_they_are_not_there_yet() {
    let (_dir, out) = run("require(\"fs\").stat(\"main.js\", () => {});\n");
    assert_eq!(out.code, Some(1));
    assert!(
        out.stderr.contains(
            "Error: Opal implements only the synchronous fs functions yet, not fs functions with a callback"
        ),
        "{}",
        out.stderr
    );
}

#[test]
fn built_in_modules_compile_from_their_code_caches() {
    // V8 refuses a cache made by another V8 or under other flags, and the
    // module then compiles from source: slower, but otherwise unnoticed.
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("main.js"),
        "require(\"fs\");\nrequire(\"assert\");\n",
    )
    .unwrap();
    let output = Command::new(OPAL)
        .current_dir(dir.path())
        .args(["run", "main.js"])
        .env("OPAL_TRACE_STARTUP", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    assert!(
        stderr.contains("opal-trace built-in code caches rejected: 0\n"),
        "{stderr}"
    );
}
