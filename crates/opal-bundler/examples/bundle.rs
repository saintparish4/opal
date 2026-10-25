//! Bundles one entry and prints the result: `cargo run -p opal-bundler --example bundle -- <root> <entry> [outdir]`.
use opal_bundler::BuildOptions;
use opal_core::graph::resolve;
use opal_core::path::NormalizedPath;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let root = NormalizedPath::from_native(&std::fs::canonicalize(&args[1]).unwrap()).unwrap();
    let options = BuildOptions::default();
    let resolution = resolve(
        &root,
        &NormalizedPath::new(&args[2]),
        &options.target.resolver_options(Some("react".into())),
    )
    .unwrap();
    let out = root
        .as_path()
        .join(args.get(3).map_or("dist", |s| s.as_str()));
    match opal_bundler::build(&resolution.graph, &out, &options) {
        Ok(output) => println!("{}", output.code),
        Err(diagnostic) => eprintln!("{diagnostic}"),
    }
}
