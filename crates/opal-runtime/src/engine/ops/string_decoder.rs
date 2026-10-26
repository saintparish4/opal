//! internalBinding('string_decoder'): Node's src/string_decoder.cc, the
//! incremental decoder under lib/string_decoder.js (vendored unmodified),
//! which streams and fs use to turn chunks of bytes into strings without
//! splitting a character between two of them.
//!
//! The decoder's state lives in a 7-byte Buffer the JavaScript owns, laid
//! out as the C++ class is: up to four bytes of an incomplete character,
//! then how many more it needs, how many it has, and the encoding (Node's
//! `enum encoding`).

use super::buffer::{Encoding, bytes, decode};
use super::{Args, Ret, string, type_error};

const INCOMPLETE_END: usize = 4;
const MISSING: usize = 4;
const BUFFERED: usize = 5;
const ENCODING: usize = 6;
pub const STATE_SIZE: usize = 7;
const _: () = assert!(INCOMPLETE_END <= MISSING);

fn encoding_of(field: u8) -> Encoding {
    match field {
        0 => Encoding::Ascii,
        1 => Encoding::Utf8,
        2 => Encoding::Base64,
        3 => Encoding::Ucs2,
        4 => Encoding::Latin1,
        5 => Encoding::Hex,
        7 => Encoding::Base64Url,
        _ => Encoding::Buffer,
    }
}

/// MakeString: `data` decoded as `encoding`, or a thrown error.
fn make_string<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    data: &[u8],
    encoding: Encoding,
) -> Option<v8::Local<'s, v8::String>> {
    let made = decode(scope, data, encoding);
    if made.is_none() {
        let message = string(
            scope,
            "Cannot create a string longer than 0x1fffffe8 characters",
        );
        let exception = v8::Exception::error(scope, message);
        let object = v8::Local::<v8::Object>::try_from(exception).unwrap();
        let key = string(scope, "code");
        let code = string(scope, "ERR_STRING_TOO_LONG");
        object.set(scope, key.into(), code.into());
        scope.throw_exception(exception);
    }
    made
}

/// StringDecoder::DecodeData.
fn decode_data<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    state: &mut [u8],
    mut data: &[u8],
) -> Option<v8::Local<'s, v8::String>> {
    let encoding = encoding_of(state[ENCODING]);
    if !matches!(
        encoding,
        Encoding::Utf8 | Encoding::Ucs2 | Encoding::Base64 | Encoding::Base64Url
    ) {
        return make_string(scope, data, encoding);
    }
    let mut prepend = None;
    // Bytes wanted to finish a character from the previous chunk.
    if state[MISSING] > 0 {
        if encoding == Encoding::Utf8 {
            // As V8's decoder would: a byte that should continue the
            // character and doesn't ends it there, and starts a new one.
            let missing = state[MISSING] as usize;
            for i in 0..data.len().min(missing) {
                if data[i] & 0xC0 != 0x80 {
                    state[MISSING] = 0;
                    let buffered = state[BUFFERED] as usize;
                    state[buffered..buffered + i].copy_from_slice(&data[..i]);
                    state[BUFFERED] += i as u8;
                    data = &data[i..];
                    break;
                }
            }
        }
        let found = data.len().min(state[MISSING] as usize);
        let buffered = state[BUFFERED] as usize;
        state[buffered..buffered + found].copy_from_slice(&data[..found]);
        data = &data[found..];
        state[MISSING] -= found as u8;
        state[BUFFERED] += found as u8;
        if state[MISSING] == 0 {
            let buffered = state[BUFFERED] as usize;
            let character = state[..buffered].to_vec();
            prepend = Some(make_string(scope, &character, encoding)?);
            state[BUFFERED] = 0;
        }
    }
    let body = if data.is_empty() {
        prepend.take().unwrap_or_else(|| v8::String::empty(scope))
    } else {
        let mut nread = data.len();
        if encoding == Encoding::Utf8 && data[nread - 1] & 0x80 != 0 {
            // Ended on a non-ASCII byte: find where its character starts.
            let mut i = nread - 1;
            loop {
                state[BUFFERED] += 1;
                if data[i] & 0xC0 == 0x80 {
                    // A trailing byte.
                    if state[BUFFERED] >= 4 || i == 0 {
                        state[BUFFERED] = 0;
                        break;
                    }
                } else {
                    // The first byte of a character tells its length.
                    state[MISSING] = if data[i] & 0xE0 == 0xC0 {
                        2
                    } else if data[i] & 0xF0 == 0xE0 {
                        3
                    } else if data[i] & 0xF8 == 0xF0 {
                        4
                    } else {
                        state[BUFFERED] = 0;
                        break;
                    };
                    if state[BUFFERED] >= state[MISSING] {
                        state[MISSING] = 0;
                        state[BUFFERED] = 0;
                    }
                    state[MISSING] -= state[BUFFERED];
                    break;
                }
                i -= 1;
            }
        } else if encoding == Encoding::Ucs2 {
            if nread % 2 == 1 {
                // Half a code unit.
                state[BUFFERED] = 1;
                state[MISSING] = 1;
            } else if data[nread - 1] & 0xFC == 0xD8 {
                // Half a surrogate pair.
                state[BUFFERED] = 2;
                state[MISSING] = 2;
            }
        } else if matches!(encoding, Encoding::Base64 | Encoding::Base64Url) {
            state[BUFFERED] = (nread % 3) as u8;
            if state[BUFFERED] > 0 {
                state[MISSING] = 3 - state[BUFFERED];
            }
        }
        let buffered = state[BUFFERED] as usize;
        if buffered > 0 {
            nread -= buffered;
            state[..buffered].copy_from_slice(&data[nread..nread + buffered]);
        }
        if nread > 0 {
            make_string(scope, &data[..nread], encoding)?
        } else {
            v8::String::empty(scope)
        }
    };
    Some(match prepend {
        Some(prepend) => v8::String::concat(scope, prepend, body)?,
        None => body,
    })
}

/// StringDecoder::FlushData.
fn flush_data<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    state: &mut [u8],
) -> Option<v8::Local<'s, v8::String>> {
    let encoding = encoding_of(state[ENCODING]);
    if encoding == Encoding::Ucs2 && state[BUFFERED] % 2 == 1 {
        // A single trailing byte is dropped, as the JS decoder drops it.
        state[MISSING] = state[MISSING].wrapping_sub(1);
        state[BUFFERED] -= 1;
    }
    if state[BUFFERED] == 0 {
        return Some(v8::String::empty(scope));
    }
    let buffered = state[BUFFERED] as usize;
    let character = state[..buffered].to_vec();
    state[MISSING] = 0;
    state[BUFFERED] = 0;
    make_string(scope, &character, encoding)
}

/// The decoder state Buffer argument, if it is one of the right size.
fn state<'a>(scope: &mut v8::PinScope, value: v8::Local<v8::Value>) -> Option<&'a mut [u8]> {
    let state = bytes(scope, value).filter(|state| state.len() >= STATE_SIZE);
    if state.is_none() {
        type_error(scope, "the decoder's state must be a Buffer of kSize bytes");
    }
    state
}

/// `decode(state, chunk)`.
pub fn string_decoder_decode(scope: &mut v8::PinScope, args: Args, mut rv: Ret) {
    let Some(state) = state(scope, args.get(0)) else {
        return;
    };
    if !args.get(1).is_array_buffer_view() {
        return type_error(scope, "the chunk must be an ArrayBufferView");
    }
    // Copied, so the decoder's state and the chunk can't be the same bytes.
    let chunk = bytes(scope, args.get(1))
        .map(|b| b.to_vec())
        .unwrap_or_default();
    if let Some(text) = decode_data(scope, state, &chunk) {
        rv.set(text.into());
    }
}

/// `flush(state)`.
pub fn string_decoder_flush(scope: &mut v8::PinScope, args: Args, mut rv: Ret) {
    let Some(state) = state(scope, args.get(0)) else {
        return;
    };
    if let Some(text) = flush_data(scope, state) {
        rv.set(text.into());
    }
}
