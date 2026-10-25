// Shared by both build scripts: the JavaScript each spike runs before the
// user's script, written to OUT_DIR/bootstrap.js.
//
// The core is a console with enough of util.inspect's formatting for the
// workloads (primitives as String() prints them). SPIKE_PAD_KB adds that
// many kilobytes of functions that are defined and never called, standing in
// for the rest of a runtime's JavaScript (path, Buffer, the module loader),
// so the cost of bootstrap size can be measured with and without a snapshot
// instead of guessed.

fn write_bootstrap() -> usize {
    println!("cargo:rerun-if-env-changed=SPIKE_PAD_KB");
    let pad_kb: usize = std::env::var("SPIKE_PAD_KB")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let mut js = String::from(
        r#"(function () {
  "use strict";
  const print = globalThis.__print;
  delete globalThis.__print;
  const format = (v) => typeof v === "string" ? v : String(v);
  globalThis.console = {
    log: (...args) => print(args.map(format).join(" ")),
    error: (...args) => print(args.map(format).join(" ")),
  };
"#,
    );
    js.push_str("  const internals = {};\n");
    let mut i = 0;
    while js.len() < pad_kb * 1024 {
        js.push_str(&format!(
            "  internals.f{i} = function f{i}(a, b) {{ const x = a * {i} + b; if (x > {i}) {{ return [x, String(x), b + 'k{i}'] }} for (let j = 0; j < b; j++) {{ a += j % {m} }} return {{ x, a, tag: 'f{i}' }} }};\n",
            m = i % 7 + 2
        ));
        i += 1;
    }
    js.push_str("  globalThis.__internals = internals;\n})();\n");
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
    std::fs::write(out.join("bootstrap.js"), &js).unwrap();
    js.len()
}
