//! `graph::node_resolution` on fixed trees, with what Node 24.19.0 answers
//! for each (tests/node-resolution-cross-check.rs asks a live node on
//! generated ones).

use std::path::{Path, PathBuf};

use opal_core::graph::node_resolution::{Failure, NodeResolver, Resolved};

fn tree(files: &[(&str, &str)]) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    for (path, text) in files {
        let path = dir.path().join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    let root = std::fs::canonicalize(dir.path()).unwrap();
    (dir, root)
}

fn resolver() -> NodeResolver {
    NodeResolver::new(Path::new("/nonexistent/bin/opal"))
}

#[test]
fn require_probes_js_json_node_then_a_directory() {
    let (_dir, root) = tree(&[
        ("main.js", ""),
        ("a.json", "{}"),
        ("lib/package.json", r#"{ "main": "./entry" }"#),
        ("lib/entry.js", ""),
    ]);
    let main = root.join("main.js");
    let mut r = resolver();
    assert_eq!(
        r.require("./a", Some(&main), false),
        Ok(root.join("a.json"))
    );
    assert_eq!(
        r.require("./lib", Some(&main), false),
        Ok(root.join("lib/entry.js"))
    );
    assert_eq!(
        r.require("./nope", Some(&main), false),
        Err(Failure::NotFound)
    );
}

#[test]
fn exports_decide_what_a_package_exposes() {
    let (_dir, root) = tree(&[
        ("main.js", ""),
        (
            "node_modules/p/package.json",
            r#"{ "exports": { ".": { "import": "./esm.mjs", "require": "./cjs.js" }, "./x/*": "./lib/*.js", "./blocked": null } }"#,
        ),
        ("node_modules/p/cjs.js", ""),
        ("node_modules/p/esm.mjs", ""),
        ("node_modules/p/lib/y.js", ""),
    ]);
    let main = root.join("main.js");
    let mut r = resolver();
    assert_eq!(
        r.require("p", Some(&main), false),
        Ok(root.join("node_modules/p/cjs.js"))
    );
    assert_eq!(
        r.import("p", &main),
        Ok(Resolved::File(root.join("node_modules/p/esm.mjs")))
    );
    assert_eq!(
        r.require("p/x/y", Some(&main), false),
        Ok(root.join("node_modules/p/lib/y.js"))
    );
    let Err(Failure::Node { code, .. }) = r.require("p/blocked", Some(&main), false) else {
        panic!("a null target blocks the path");
    };
    assert_eq!(code, "ERR_PACKAGE_PATH_NOT_EXPORTED");
}

#[test]
fn an_import_needs_the_extension_and_says_what_require_would_have_found() {
    let (_dir, root) = tree(&[
        ("main.mjs", ""),
        ("node_modules/p/lib/x.js", ""),
        ("node_modules/p/package.json", "{}"),
    ]);
    let main = root.join("main.mjs");
    let mut r = resolver();
    let Err(Failure::Node { code, hint, .. }) = r.import("p/lib/x", &main) else {
        panic!("imports don't add extensions");
    };
    assert_eq!(code, "ERR_MODULE_NOT_FOUND");
    assert_eq!(
        hint.as_deref(),
        Some("Did you mean to import \"p/lib/x.js\"?")
    );
}

#[test]
fn a_package_can_require_itself_by_name() {
    let (_dir, root) = tree(&[
        (
            "package.json",
            r#"{ "name": "me", "exports": { "./util": "./util.js" } }"#,
        ),
        ("src/main.js", ""),
        ("util.js", ""),
    ]);
    let mut r = resolver();
    assert_eq!(
        r.require("me/util", Some(&root.join("src/main.js")), false),
        Ok(root.join("util.js"))
    );
}
