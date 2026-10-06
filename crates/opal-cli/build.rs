//! Records the commit a build came from, for the line `opal install` opens
//! with. Two builds of one version are otherwise indistinguishable, which is
//! how a benchmark of "0.3.1" can turn out to be of something else.

use std::path::Path;
use std::process::Command;

fn main() {
    // A source tarball or a vendored copy has no git directory. The header
    // then names the version alone, and the build does not fail over it.
    let commit = Command::new("git")
        .args(["rev-parse", "--short=7", "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|commit| commit.trim().to_string())
        .unwrap_or_default();
    println!("cargo:rustc-env=OPAL_COMMIT={commit}");

    println!("cargo:rerun-if-changed=build.rs");
    // HEAD only changes on a checkout; its reflog grows on every commit too.
    // Named only when present: a path that does not exist reruns this script
    // on every build.
    for watched in ["../../.git/HEAD", "../../.git/logs/HEAD"] {
        if Path::new(watched).exists() {
            println!("cargo:rerun-if-changed={watched}");
        }
    }
}
