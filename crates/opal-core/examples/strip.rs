//! Strips the types from a file and prints the result: a way to look at what
//! `typescript::strip_types` does to a given input.
//!
//!     cargo run -p opal-core --example strip -- file.ts

fn main() {
    let path = std::env::args().nth(1).expect("usage: strip <file.ts>");
    let source = std::fs::read_to_string(&path).expect("read the file");
    match opal_core::typescript::strip_types(&source) {
        Ok(output) => print!("{output}"),
        Err(error) => {
            eprintln!(
                "{}: {} ({}..{})",
                error.kind.code(),
                error.message,
                error.start,
                error.end
            );
            std::process::exit(1);
        }
    }
}
