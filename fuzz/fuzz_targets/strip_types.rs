//! TypeScript source through the type stripper `opal run` runs every .ts file
//! through (`opal_core::typescript::strip_types`).
//!
//! The source is whatever is on disk, so the stripper has to survive any
//! input. And what it promises when it succeeds is what keeps a stack trace's
//! line and column the source's: the output is exactly as long as the input,
//! every line break is where it was, and every byte that changed is one the
//! stripper writes (a space, the first byte of a multi-byte whitespace
//! character, or the `;`, `(` and `)` it moves to keep statements apart).
//!
//! ```text
//! cargo +nightly fuzz run strip_types corpus/strip_types seeds/strip_types -- -dict=strip_types.dict -timeout=10
//! ```

#![no_main]

use libfuzzer_sys::fuzz_target;
use opal_core::typescript::strip_types;

fn is_break(bytes: &[u8], i: usize) -> bool {
    bytes[i] == b'\n'
        || bytes[i] == b'\r'
        || bytes[i..].starts_with("\u{2028}".as_bytes())
        || bytes[i..].starts_with("\u{2029}".as_bytes())
}

fuzz_target!(|data: &[u8]| {
    let Ok(source) = std::str::from_utf8(data) else {
        return;
    };
    let Ok(output) = strip_types(source) else {
        return;
    };
    let (input, out) = (source.as_bytes(), output.as_bytes());
    assert_eq!(input.len(), out.len(), "stripping moved bytes");
    for i in 0..input.len() {
        assert_eq!(is_break(input, i), is_break(out, i), "a line break moved at {i}");
        if input[i] != out[i] {
            let written = matches!(out[i], b' ' | b';' | b'(' | b')')
                || (out[i] >= 0x80 && !input[i].is_ascii_whitespace());
            assert!(written, "byte {i} became {:?}, which the stripper doesn't write", out[i] as char);
        }
    }
    // What comes out is JavaScript; stripping it again must not panic.
    let _ = strip_types(&output);
});
