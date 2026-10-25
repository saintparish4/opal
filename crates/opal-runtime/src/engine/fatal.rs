//! What Node prints when an error ends the process.
//!
//! ```text
//! /app/main.js:3
//!   null.x;
//!        ^
//!
//! TypeError: Cannot read properties of null (reading 'x')
//!     at f (/app/main.js:3:8)
//!
//! Opal v0.4.1
//! ```
//!
//! The first three lines (the "arrow") come from V8's message for the
//! exception: for a `throw`, the throw site; for a rejected promise, where
//! the error was constructed. A value that is not a native error (`throw {}`)
//! gets the arrow first, after a blank line, and no blank line before the
//! value, which is how node_errors.cc orders the two cases. The last line
//! names Opal rather than Node.js, the one place the output differs on purpose.

use super::ops::write_all;

pub(super) fn report(
    scope: &mut v8::PinScope,
    error: v8::Local<v8::Value>,
    message: Option<v8::Local<v8::Message>>,
    formatted: &str,
) {
    let arrow = message.and_then(|message| arrow(scope, message));
    let text = match (arrow, error.is_native_error()) {
        (Some(arrow), true) => format!("{arrow}\n{formatted}\n"),
        (Some(arrow), false) => format!("\n{arrow}{formatted}\n"),
        (None, _) => format!("{formatted}\n"),
    };
    let version = env!("CARGO_PKG_VERSION");
    let _ = write_all(2, format!("{text}\nOpal v{version}\n").as_bytes());
}

/// `file:line`, the source line, and carets under the part that threw, with
/// tabs in the source line kept so the carets line up.
fn arrow(scope: &mut v8::PinScope, message: v8::Local<v8::Message>) -> Option<String> {
    let resource = message.get_script_resource_name(scope)?;
    if resource.is_undefined() {
        return None;
    }
    let resource = resource.to_rust_string_lossy(scope);
    let line = message.get_line_number(scope)?;
    let source = message.get_source_line(scope)?.to_rust_string_lossy(scope);
    let start = message.get_start_column();
    let end = message.get_end_column().max(start + 1);
    let mut underline: String = source
        .chars()
        .take(start)
        .map(|c| if c == '\t' { '\t' } else { ' ' })
        .collect();
    underline.extend(std::iter::repeat_n('^', end - start));
    Some(format!("{resource}:{line}\n{source}\n{underline}\n"))
}
