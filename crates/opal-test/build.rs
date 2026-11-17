//! Embeds every file under `runtime/` into the binary, so a runtime file
//! can't be left out of a list by hand.

use std::fmt::Write;
use std::path::Path;

fn main() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("runtime");
    println!("cargo:rerun-if-changed={}", dir.display());
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .expect("runtime/ exists")
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".cjs"))
        .collect();
    names.sort();
    let mut out = String::from("&[\n");
    for name in &names {
        let path = dir.join(name);
        println!("cargo:rerun-if-changed={}", path.display());
        writeln!(
            out,
            "    ({name:?}, include_str!({:?})),",
            path.display().to_string()
        )
        .expect("write");
    }
    out.push(']');
    let target = Path::new(&std::env::var("OUT_DIR").expect("OUT_DIR")).join("runtime_files.rs");
    std::fs::write(target, out).expect("write runtime_files.rs");
}
