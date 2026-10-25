include!("../bootstrap.rs");

// SPIKE_V8_SNAPSHOT names a blob written by `spike-v8 --write-snapshot`,
// embedded so that start-up reads no file. Unset, the blob is empty and the
// bootstrap runs on every start. SPIKE_V8_FLAGS is passed to V8 before it
// starts (e.g. --jitless).
fn main() {
    let size = write_bootstrap();
    println!("cargo:rustc-env=SPIKE_BOOTSTRAP_BYTES={size}");
    println!("cargo:rerun-if-env-changed=SPIKE_V8_SNAPSHOT");
    println!("cargo:rerun-if-env-changed=SPIKE_V8_FLAGS");
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let blob = match std::env::var("SPIKE_V8_SNAPSHOT") {
        Ok(path) => {
            println!("cargo:rerun-if-changed={path}");
            std::fs::read(path).expect("read SPIKE_V8_SNAPSHOT")
        }
        Err(_) => Vec::new(),
    };
    std::fs::write(out.join("snapshot.bin"), blob).unwrap();
    let flags = std::env::var("SPIKE_V8_FLAGS").unwrap_or_default();
    println!("cargo:rustc-env=SPIKE_V8_FLAGS={flags}");
}
