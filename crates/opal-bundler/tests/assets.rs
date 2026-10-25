//! SEMANTICS.md rule 17: `--assets`. No runtime runs these programs
//! unbundled (Node refuses a stylesheet or an image import), so this checks
//! what the build writes and refuses, not two runs; the compatibility
//! matrix loads a real one in Chromium.

use opal_bundler::{BuildOptions, Target};
use opal_core::graph::resolve;
use opal_core::path::NormalizedPath;

fn build(
    files: &[(&str, &str)],
    options: &BuildOptions,
) -> Result<opal_bundler::BuildOutput, opal_bundler::Diagnostic> {
    let dir = tempfile::tempdir().unwrap();
    for (path, text) in files {
        std::fs::write(dir.path().join(path), text).unwrap();
    }
    let root = NormalizedPath::from_native(&std::fs::canonicalize(dir.path()).unwrap()).unwrap();
    let resolution = resolve(
        &root,
        &NormalizedPath::new("main.mjs"),
        &options.target.resolver_options(None),
    )
    .unwrap();
    opal_bundler::build(&resolution.graph, &dir.path().join("dist"), options)
}

#[test]
fn test_an_asset_import_is_the_url_of_a_hashed_copy() {
    let options = BuildOptions {
        target: Target::Browser,
        assets: true,
        ..BuildOptions::default()
    };
    let output = build(
        &[
            (
                "main.mjs",
                "import logo from './logo.svg';\nimport './app.css';\nconsole.log(logo);\n",
            ),
            ("logo.svg", "<svg/>"),
            ("app.css", "body { color: red }"),
        ],
        &options,
    )
    .unwrap();
    let names: Vec<&str> = output
        .assets
        .iter()
        .map(|(name, _)| name.as_str())
        .collect();
    assert_eq!(names.len(), 2);
    let svg = names.iter().find(|n| n.ends_with(".svg")).unwrap();
    assert!(svg.starts_with("assets/logo-"), "{svg}");
    assert!(output.code.contains(&format!("\"./{svg}\"")));
    assert!(
        output.code.contains("css(__opal_asset"),
        "the stylesheet is linked at its step"
    );
    assert_eq!(
        output
            .assets
            .iter()
            .find(|(n, _)| n.ends_with(".svg"))
            .unwrap()
            .1,
        b"<svg/>"
    );
}

#[test]
fn test_assets_are_refused_without_the_flag_and_where_they_cannot_work() {
    let files = [
        (
            "main.mjs",
            "import logo from './logo.svg';\nconsole.log(logo);\n",
        ),
        ("logo.svg", "<svg/>"),
    ];
    let refused = build(&files, &BuildOptions::default()).err().unwrap();
    assert_eq!(refused.code.to_string(), "E-UNSUPPORTED");

    let node_css = BuildOptions {
        assets: true,
        ..BuildOptions::default()
    };
    let refused = build(
        &[("main.mjs", "import './a.css';\n"), ("a.css", "a {}")],
        &node_css,
    )
    .err()
    .unwrap();
    assert_eq!(refused.code.to_string(), "E-UNSUPPORTED");

    let browser = BuildOptions {
        target: Target::Browser,
        assets: true,
        ..BuildOptions::default()
    };
    let refused = build(
        &[
            ("main.mjs", "import './a.css';\n"),
            ("a.css", "a { background: url(./x.png) }"),
        ],
        &browser,
    )
    .err()
    .unwrap();
    assert_eq!(refused.code.to_string(), "E-UNSUPPORTED");
    assert!(refused.message.contains("url()"));
}
