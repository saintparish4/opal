//! quickjs-ng through rquickjs: run the bootstrap, then the script named on
//! the command line, then any pending jobs. No snapshot: QuickJS has none,
//! so the bootstrap is parsed and compiled to bytecode on every start.

use std::io::Write;

use rquickjs::{Context, Function, Runtime};

const BOOTSTRAP: &str = include_str!(concat!(env!("OUT_DIR"), "/bootstrap.js"));

fn main() {
    let path = std::env::args().nth(1).expect("usage: spike-quickjs <script.js>");
    let source = std::fs::read_to_string(&path).expect("read script");
    let runtime = Runtime::new().expect("runtime");
    let context = Context::full(&runtime).expect("context");
    let failed = context.with(|ctx| {
        let print = Function::new(ctx.clone(), |line: String| {
            let mut out = std::io::stdout().lock();
            let _ = out.write_all(line.as_bytes());
            let _ = out.write_all(b"\n");
        })
        .expect("print");
        ctx.globals().set("__print", print).expect("global");
        ctx.eval::<(), _>(BOOTSTRAP).expect("bootstrap");
        match ctx.eval::<(), _>(source) {
            Ok(()) => false,
            Err(error) => {
                let detail = ctx.catch();
                eprintln!("{error}: {detail:?}");
                true
            }
        }
    });
    while runtime.is_job_pending() {
        if runtime.execute_pending_job().is_err() {
            break;
        }
    }
    let _ = std::io::stdout().flush();
    std::process::exit(i32::from(failed));
}
