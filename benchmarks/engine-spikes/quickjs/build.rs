include!("../bootstrap.rs");

fn main() {
    let size = write_bootstrap();
    println!("cargo:rustc-env=SPIKE_BOOTSTRAP_BYTES={size}");
}
