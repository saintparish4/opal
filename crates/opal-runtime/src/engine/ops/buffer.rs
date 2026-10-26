//! internalBinding('buffer'): the byte-level work behind Node's lib/buffer.js
//! (vendored unmodified in js/node/), with the semantics of Node's
//! src/node_buffer.cc and src/string_bytes.cc, including their edge cases:
//! hex decoding stops at the first invalid pair, base64 decoding skips
//! characters outside the alphabet and stops at `=` (nbytes' decoder, which
//! Node falls back to whenever simdutf's strict one refuses), UTF-8 encoding
//! writes only whole characters and replaces lone surrogates, and decoders
//! of two-byte strings read each UTF-16 unit's low byte, as the C++ does by
//! casting to uint8_t.

use super::{Args, Ret, string, type_error};

/// Node's `enum encoding` (src/node.h), as internal/util's encodingsMap
/// numbers them.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Encoding {
    Ascii = 0,
    Utf8 = 1,
    Base64 = 2,
    Ucs2 = 3,
    Latin1 = 4,
    Hex = 5,
    Buffer = 6,
    Base64Url = 7,
}

impl Encoding {
    pub(super) fn from_value(scope: &mut v8::PinScope, value: v8::Local<v8::Value>) -> Encoding {
        if value.is_number() {
            return match value.int32_value(scope).unwrap_or(1) {
                0 => Encoding::Ascii,
                2 => Encoding::Base64,
                3 => Encoding::Ucs2,
                4 => Encoding::Latin1,
                5 => Encoding::Hex,
                6 => Encoding::Buffer,
                7 => Encoding::Base64Url,
                _ => Encoding::Utf8,
            };
        }
        if !value.is_string() {
            return Encoding::Utf8;
        }
        match value
            .to_rust_string_lossy(scope)
            .to_ascii_lowercase()
            .as_str()
        {
            "ascii" => Encoding::Ascii,
            "base64" => Encoding::Base64,
            "base64url" => Encoding::Base64Url,
            "ucs2" | "ucs-2" | "utf16le" | "utf-16le" => Encoding::Ucs2,
            "latin1" | "binary" => Encoding::Latin1,
            "hex" => Encoding::Hex,
            "buffer" => Encoding::Buffer,
            _ => Encoding::Utf8,
        }
    }
}

/// The bytes a typed array, DataView or ArrayBuffer views. Valid until
/// JavaScript runs again, which no op here lets happen while it holds them.
pub(super) fn bytes<'a>(
    scope: &mut v8::PinScope,
    value: v8::Local<v8::Value>,
) -> Option<&'a mut [u8]> {
    let (buffer, offset, length) =
        if let Ok(view) = v8::Local::<v8::ArrayBufferView>::try_from(value) {
            (view.buffer(scope)?, view.byte_offset(), view.byte_length())
        } else if let Ok(buffer) = v8::Local::<v8::ArrayBuffer>::try_from(value) {
            let length = buffer.byte_length();
            (buffer, 0, length)
        } else {
            return None;
        };
    if length == 0 {
        return Some(&mut []);
    }
    let data = buffer.data()?;
    // SAFETY: the backing store holds byte_offset + byte_length bytes, and
    // stays put while the view is alive and no JavaScript runs.
    Some(unsafe { std::slice::from_raw_parts_mut(data.as_ptr().cast::<u8>().add(offset), length) })
}

fn view_bytes<'a>(scope: &mut v8::PinScope, args: &Args, index: i32) -> Option<&'a mut [u8]> {
    let value = args.get(index);
    let found = bytes(scope, value);
    if found.is_none() {
        type_error(scope, "argument must be a buffer");
    }
    found
}

fn utf16_units(scope: &mut v8::PinScope, text: v8::Local<v8::String>) -> Vec<u16> {
    let mut units = vec![0; text.length()];
    text.write_v2(scope, 0, &mut units, v8::WriteFlags::empty());
    units
}

/// A string's bytes in `encoding`, as StringBytes::Write would write them
/// with unlimited room.
pub(super) fn encode(
    scope: &mut v8::PinScope,
    text: v8::Local<v8::String>,
    encoding: Encoding,
) -> Vec<u8> {
    match encoding {
        Encoding::Utf8 | Encoding::Buffer => {
            let mut out = vec![0; text.utf8_length(scope)];
            let written =
                text.write_utf8_v2(scope, &mut out, v8::WriteFlags::kReplaceInvalidUtf8, None);
            out.truncate(written);
            out
        }
        Encoding::Ucs2 => utf16_units(scope, text)
            .iter()
            .flat_map(|u| u.to_le_bytes())
            .collect(),
        Encoding::Latin1 | Encoding::Ascii => {
            let mut out = vec![0; text.length()];
            text.write_one_byte_v2(scope, 0, &mut out, v8::WriteFlags::empty());
            out
        }
        Encoding::Hex => {
            let units = utf16_units(scope, text);
            let mut out = vec![0; units.len() / 2];
            let written = hex_decode(&mut out, &units);
            out.truncate(written);
            out
        }
        Encoding::Base64 | Encoding::Base64Url => {
            let units = utf16_units(scope, text);
            let mut out = vec![0; base64_decoded_size(&units)];
            let written = base64_decode(&mut out, &units);
            out.truncate(written);
            out
        }
    }
}

/// Writes `text` in `encoding` into `dst`, as much as fits, returning the
/// number of bytes written: StringBytes::Write.
fn write_into(
    scope: &mut v8::PinScope,
    dst: &mut [u8],
    text: v8::Local<v8::String>,
    encoding: Encoding,
) -> usize {
    match encoding {
        Encoding::Utf8 | Encoding::Buffer => {
            text.write_utf8_v2(scope, dst, v8::WriteFlags::kReplaceInvalidUtf8, None)
        }
        Encoding::Ucs2 => {
            let units = utf16_units(scope, text);
            let count = units.len().min(dst.len() / 2);
            for (i, unit) in units[..count].iter().enumerate() {
                dst[2 * i..2 * i + 2].copy_from_slice(&unit.to_le_bytes());
            }
            count * 2
        }
        Encoding::Latin1 | Encoding::Ascii => {
            let count = text.length().min(dst.len());
            text.write_one_byte_v2(scope, 0, &mut dst[..count], v8::WriteFlags::empty());
            count
        }
        Encoding::Hex => {
            let units = utf16_units(scope, text);
            hex_decode(dst, &units)
        }
        Encoding::Base64 | Encoding::Base64Url => {
            let units = utf16_units(scope, text);
            base64_decode(dst, &units)
        }
    }
}

fn unhex(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

/// nbytes::HexDecode: pairs of hex digits until the first pair that isn't
/// one, or until `dst` is full.
fn hex_decode(dst: &mut [u8], src: &[u16]) -> usize {
    let mut i = 0;
    while i < dst.len() && i * 2 + 1 < src.len() {
        let (Some(a), Some(b)) = (unhex(src[i * 2] as u8), unhex(src[i * 2 + 1] as u8)) else {
            return i;
        };
        dst[i] = (a << 4) | b;
        i += 1;
    }
    i
}

/// nbytes' unbase64: both alphabets decode, so `-` and `+` are 62 and `_`
/// and `/` are 63. Anything else is 255.
fn unbase64(c: u8) -> u8 {
    match c {
        b'A'..=b'Z' => c - b'A',
        b'a'..=b'z' => c - b'a' + 26,
        b'0'..=b'9' => c - b'0' + 52,
        b'+' | b'-' => 62,
        b'/' | b'_' => 63,
        _ => 255,
    }
}

fn base64_decoded_size_fast(size: usize) -> usize {
    // 1-byte input cannot be decoded.
    if size > 1 {
        (size / 4) * 3 + (size % 4).div_ceil(2)
    } else {
        0
    }
}

/// nbytes::Base64DecodedSize: the length without up to two trailing `=`.
fn base64_decoded_size(src: &[u16]) -> usize {
    let mut size = src.len();
    if size < 2 {
        return 0;
    }
    if src[size - 1] == u16::from(b'=') {
        size -= 1;
        if src[size - 1] == u16::from(b'=') {
            size -= 1;
        }
    }
    base64_decoded_size_fast(size)
}

/// One group of four characters, skipping ones outside the alphabet; false
/// means stop decoding (at `=`, at the end of the input, or a full `dst`).
fn base64_decode_group_slow(dst: &mut [u8], src: &[u16], i: &mut usize, k: &mut usize) -> bool {
    let mut hi = 0;
    for step in 0..4 {
        let lo = loop {
            let c = src[*i] as u8;
            let lo = unbase64(c);
            *i += 1;
            if lo < 64 {
                break lo;
            }
            if c == b'=' || *i >= src.len() {
                return false;
            }
        };
        match step {
            1 => {
                dst[*k] = ((hi & 0x3F) << 2) | ((lo & 0x30) >> 4);
                *k += 1;
            }
            2 => {
                dst[*k] = ((hi & 0x0F) << 4) | ((lo & 0x3C) >> 2);
                *k += 1;
            }
            3 => {
                dst[*k] = ((hi & 0x03) << 6) | (lo & 0x3F);
                *k += 1;
            }
            _ => {}
        }
        if *i >= src.len() || *k >= dst.len() {
            return false;
        }
        hi = lo;
    }
    true
}

/// nbytes::Base64Decode.
fn base64_decode(dst: &mut [u8], src: &[u16]) -> usize {
    let decoded_size = base64_decoded_size(src);
    let available = dst.len().min(decoded_size);
    let max_k = available / 3 * 3;
    let mut max_i = src.len() / 4 * 4;
    let mut i = 0;
    let mut k = 0;
    while i < max_i && k < max_k {
        let txt = [
            unbase64(src[i] as u8),
            unbase64(src[i + 1] as u8),
            unbase64(src[i + 2] as u8),
            unbase64(src[i + 3] as u8),
        ];
        let v = u32::from_be_bytes(txt);
        // A set high bit means whitespace or a character outside the alphabet.
        if v & 0x8080_8080 != 0 {
            if !base64_decode_group_slow(dst, src, &mut i, &mut k) {
                return k;
            }
            max_i = i + (src.len() - i) / 4 * 4;
        } else {
            dst[k] = (((v >> 22) & 0xFC) | ((v >> 20) & 0x03)) as u8;
            dst[k + 1] = (((v >> 12) & 0xF0) | ((v >> 10) & 0x0F)) as u8;
            dst[k + 2] = (((v >> 2) & 0xC0) | (v & 0x3F)) as u8;
            i += 4;
            k += 3;
        }
    }
    if i < src.len() && k < dst.len() {
        base64_decode_group_slow(dst, src, &mut i, &mut k);
    }
    k
}

const BASE64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const BASE64URL: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

fn base64_encode(src: &[u8], table: &[u8; 64], pad: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(src.len().div_ceil(3) * 4);
    for chunk in src.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(table[(n >> 18) as usize & 63]);
        out.push(table[(n >> 12) as usize & 63]);
        if chunk.len() > 1 {
            out.push(table[(n >> 6) as usize & 63]);
        } else if pad {
            out.push(b'=');
        }
        if chunk.len() > 2 {
            out.push(table[n as usize & 63]);
        } else if pad {
            out.push(b'=');
        }
    }
    out
}

/// A string of `bytes` decoded as `encoding`: StringBytes::Encode.
pub(super) fn decode<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    bytes: &[u8],
    encoding: Encoding,
) -> Option<v8::Local<'s, v8::String>> {
    match encoding {
        Encoding::Utf8 | Encoding::Buffer => {
            // from_utf8_lossy replaces each maximal ill-formed subsequence
            // with U+FFFD, the WHATWG decoder's rule, which Node follows.
            let text = String::from_utf8_lossy(bytes);
            v8::String::new_from_utf8(scope, text.as_bytes(), v8::NewStringType::Normal)
        }
        Encoding::Latin1 => v8::String::new_from_one_byte(scope, bytes, v8::NewStringType::Normal),
        Encoding::Ascii => {
            let masked: Vec<u8> = bytes.iter().map(|b| b & 0x7F).collect();
            v8::String::new_from_one_byte(scope, &masked, v8::NewStringType::Normal)
        }
        Encoding::Ucs2 => {
            let units: Vec<u16> = bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|p| u16::from_le_bytes(*p))
                .collect();
            v8::String::new_from_two_byte(scope, &units, v8::NewStringType::Normal)
        }
        Encoding::Hex => {
            const DIGITS: &[u8; 16] = b"0123456789abcdef";
            let hex: Vec<u8> = bytes
                .iter()
                .flat_map(|b| [DIGITS[(b >> 4) as usize], DIGITS[(b & 15) as usize]])
                .collect();
            v8::String::new_from_one_byte(scope, &hex, v8::NewStringType::Normal)
        }
        Encoding::Base64 => {
            let text = base64_encode(bytes, BASE64, true);
            v8::String::new_from_one_byte(scope, &text, v8::NewStringType::Normal)
        }
        Encoding::Base64Url => {
            let text = base64_encode(bytes, BASE64URL, false);
            v8::String::new_from_one_byte(scope, &text, v8::NewStringType::Normal)
        }
    }
}

/// Clamps `start` and `end` (numbers or undefined) to `[0, len]` the way
/// node_buffer.cc's SLICE_START_END does.
fn slice_bounds(
    scope: &mut v8::PinScope,
    args: &Args,
    first: i32,
    len: usize,
) -> Option<(usize, usize)> {
    let index =
        |scope: &mut v8::PinScope, value: v8::Local<v8::Value>, default: usize| -> Option<usize> {
            if value.is_undefined() {
                return Some(default);
            }
            let n = value.integer_value(scope)?;
            Some(if n < 0 {
                0
            } else {
                (n as u64).min(len as u64) as usize
            })
        };
    let start = index(scope, args.get(first), 0)?;
    let end = index(scope, args.get(first + 1), len)?;
    Some((start, end.max(start)))
}

/// `buffer_slice(buf, start, end, encoding)`: the asciiSlice, utf8Slice, ...
/// family.
pub fn buffer_slice(scope: &mut v8::PinScope, args: Args, mut rv: Ret) {
    let Some(data) = view_bytes(scope, &args, 0) else {
        return;
    };
    let Some((start, end)) = slice_bounds(scope, &args, 1, data.len()) else {
        return;
    };
    let encoding = Encoding::from_value(scope, args.get(3));
    match decode(scope, &data[start..end], encoding) {
        Some(text) => rv.set(text.into()),
        None => {
            let message = string(
                scope,
                "Cannot create a string longer than 0x1fffffe8 characters",
            );
            let error = v8::Exception::error(scope, message);
            let object = v8::Local::<v8::Object>::try_from(error).unwrap();
            let key = string(scope, "code");
            let code = string(scope, "ERR_STRING_TOO_LONG");
            object.set(scope, key.into(), code.into());
            scope.throw_exception(error);
        }
    }
}

/// `buffer_write(buf, string, offset, length, encoding)`: the utf8Write,
/// hexWrite, ... family; returns the number of bytes written. The bounds
/// were checked by internal/buffer.js.
pub fn buffer_write(scope: &mut v8::PinScope, args: Args, mut rv: Ret) {
    let Some(data) = view_bytes(scope, &args, 0) else {
        return;
    };
    let Ok(text) = v8::Local::<v8::String>::try_from(args.get(1)) else {
        return type_error(scope, "argument must be a string");
    };
    let len = data.len();
    let offset = (args.get(2).integer_value(scope).unwrap_or(0).max(0) as usize).min(len);
    let max = match args.get(3) {
        value if value.is_undefined() => len - offset,
        value => (value.integer_value(scope).unwrap_or(0).max(0) as usize).min(len - offset),
    };
    let encoding = Encoding::from_value(scope, args.get(4));
    let written = write_into(scope, &mut data[offset..offset + max], text, encoding);
    rv.set_uint32(written as u32);
}

/// `buffer_byte_length_utf8(string)`.
pub fn buffer_byte_length_utf8(scope: &mut v8::PinScope, args: Args, mut rv: Ret) {
    let Ok(text) = v8::Local::<v8::String>::try_from(args.get(0)) else {
        return type_error(scope, "argument must be a string");
    };
    rv.set_double(text.utf8_length(scope) as f64);
}

fn normalize_compare(value: std::cmp::Ordering, a_len: usize, b_len: usize) -> i32 {
    match value.then(a_len.cmp(&b_len)) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    }
}

/// `buffer_compare(a, b)`: -1, 0 or 1.
pub fn buffer_compare(scope: &mut v8::PinScope, args: Args, mut rv: Ret) {
    let (Some(a), Some(b)) = (view_bytes(scope, &args, 0), view_bytes(scope, &args, 1)) else {
        return;
    };
    let n = a.len().min(b.len());
    rv.set_int32(normalize_compare(a[..n].cmp(&b[..n]), a.len(), b.len()));
}

fn out_of_range(scope: &mut v8::PinScope, name: &str) {
    let message = string(scope, &format!("The value of \"{name}\" is out of range."));
    let error = v8::Exception::range_error(scope, message);
    let object = v8::Local::<v8::Object>::try_from(error).unwrap();
    let key = string(scope, "code");
    let code = string(scope, "ERR_OUT_OF_RANGE");
    object.set(scope, key.into(), code.into());
    scope.throw_exception(error);
}

/// `buffer_compare_offset(source, target, targetStart, sourceStart,
/// targetEnd, sourceEnd)`: Buffer#compare with ranges.
pub fn buffer_compare_offset(scope: &mut v8::PinScope, args: Args, mut rv: Ret) {
    let (Some(source), Some(target)) = (view_bytes(scope, &args, 0), view_bytes(scope, &args, 1))
    else {
        return;
    };
    let arg = |scope: &mut v8::PinScope, i: i32, default: usize| -> usize {
        let value = args.get(i);
        if value.is_undefined() {
            return default;
        }
        value.integer_value(scope).unwrap_or(0).max(0) as usize
    };
    let target_start = arg(scope, 2, 0);
    let source_start = arg(scope, 3, 0);
    let target_end = arg(scope, 4, target.len()).min(target.len());
    let source_end = arg(scope, 5, source.len()).min(source.len());
    if source_start > source.len() {
        return out_of_range(scope, "sourceStart");
    }
    if target_start > target.len() {
        return out_of_range(scope, "targetStart");
    }
    let source_len = source_end.saturating_sub(source_start);
    let target_len = target_end.saturating_sub(target_start);
    let n = source_len.min(target_len).min(source.len() - source_start);
    let order = source[source_start..source_start + n].cmp(&target[target_start..target_start + n]);
    rv.set_int32(normalize_compare(order, source_len, target_len));
}

/// `buffer_copy(source, target, targetStart, sourceStart, count)`: memmove;
/// lib/buffer.js has checked the ranges.
pub fn buffer_copy(scope: &mut v8::PinScope, args: Args, mut rv: Ret) {
    let (Some(source), Some(target)) = (view_bytes(scope, &args, 0), view_bytes(scope, &args, 1))
    else {
        return;
    };
    let target_start = args.get(2).integer_value(scope).unwrap_or(0).max(0) as usize;
    let source_start = args.get(3).integer_value(scope).unwrap_or(0).max(0) as usize;
    let count = args.get(4).integer_value(scope).unwrap_or(0).max(0) as usize;
    let count = count
        .min(source.len().saturating_sub(source_start))
        .min(target.len().saturating_sub(target_start));
    // SAFETY: both ranges are in bounds (clamped above); they may overlap,
    // which is why this is a copy and not two slices.
    unsafe {
        std::ptr::copy(
            source.as_ptr().add(source_start),
            target.as_mut_ptr().add(target_start),
            count,
        );
    }
    rv.set_double(count as f64);
}

/// `buffer_fill(buf, value, start, end, encoding)`: fills with a string's
/// or a buffer's bytes, repeated. -1 if the value encodes to no bytes (an
/// invalid hex string), -2 if the range is out of bounds.
pub fn buffer_fill(scope: &mut v8::PinScope, args: Args, mut rv: Ret) {
    let Some(data) = view_bytes(scope, &args, 0) else {
        return;
    };
    let start = args.get(2).integer_value(scope).unwrap_or(0).max(0) as usize;
    let end = args.get(3).integer_value(scope).unwrap_or(0).max(0) as usize;
    if start > end || end > data.len() {
        return rv.set_int32(-2);
    }
    let fill = &mut data[start..end];
    let value = args.get(1);
    let pattern = if let Some(source) = bytes(scope, value) {
        source.to_vec()
    } else if let Ok(text) = v8::Local::<v8::String>::try_from(value) {
        let encoding = Encoding::from_value(scope, args.get(4));
        encode(scope, text, encoding)
    } else {
        return type_error(scope, "fill value must be a string or a buffer");
    };
    if pattern.len() >= fill.len() {
        fill.copy_from_slice(&pattern[..fill.len()]);
        return;
    }
    if pattern.is_empty() {
        return rv.set_int32(-1);
    }
    for (i, byte) in fill.iter_mut().enumerate() {
        *byte = pattern[i % pattern.len()];
    }
}

/// `buffer_is_utf8(view)` and `buffer_is_ascii(view)`.
pub fn buffer_is_utf8(scope: &mut v8::PinScope, args: Args, mut rv: Ret) {
    if let Some(data) = view_bytes(scope, &args, 0) {
        rv.set_bool(std::str::from_utf8(data).is_ok());
    }
}

pub fn buffer_is_ascii(scope: &mut v8::PinScope, args: Args, mut rv: Ret) {
    if let Some(data) = view_bytes(scope, &args, 0) {
        rv.set_bool(data.is_ascii());
    }
}

/// node_buffer.cc's IndexOfOffset: where a search starts, from a byteOffset
/// that may be negative (from the end) or past either end.
fn index_of_offset(length: usize, offset: i64, needle_length: usize, forward: bool) -> i64 {
    let length = length as i64;
    let needle_length = needle_length as i64;
    if offset < 0 {
        if offset + length >= 0 {
            // Negative offsets count backwards from the end.
            length + offset
        } else if forward || needle_length == 0 {
            // indexOf from before the start: search the whole buffer.
            0
        } else {
            // lastIndexOf from before the start: no match.
            -1
        }
    } else if offset + needle_length <= length {
        offset
    } else if needle_length == 0 {
        // Past the end with an empty needle: the end.
        length
    } else if forward {
        // indexOf from past the end: no match.
        -1
    } else {
        // lastIndexOf from past the end: search the whole buffer.
        length - 1
    }
}

/// The first match at or after `offset` (forward), or the last match that
/// starts at or before it (backward), in units of `T`.
fn search<T: PartialEq>(
    haystack: &[T],
    needle: &[T],
    offset: usize,
    forward: bool,
) -> Option<usize> {
    if needle.len() > haystack.len() {
        return None;
    }
    let last = haystack.len() - needle.len();
    if forward {
        (offset..=last).find(|&i| haystack[i..i + needle.len()] == *needle)
    } else {
        (0..=offset.min(last))
            .rev()
            .find(|&i| haystack[i..i + needle.len()] == *needle)
    }
}

fn index_of_bytes(
    haystack: &[u8],
    needle: &[u8],
    offset: i64,
    encoding: Encoding,
    forward: bool,
) -> f64 {
    let start = index_of_offset(haystack.len(), offset, needle.len(), forward);
    if needle.is_empty() {
        return start as f64;
    }
    if haystack.is_empty() || start <= -1 {
        return -1.0;
    }
    let start = start as usize;
    if (forward && needle.len() + start > haystack.len()) || needle.len() > haystack.len() {
        return -1.0;
    }
    let found = if encoding == Encoding::Ucs2 {
        if haystack.len() < 2 || needle.len() < 2 {
            return -1.0;
        }
        let hay: Vec<u16> = haystack
            .as_chunks::<2>()
            .0
            .iter()
            .map(|p| u16::from_le_bytes(*p))
            .collect();
        let pin: Vec<u16> = needle
            .as_chunks::<2>()
            .0
            .iter()
            .map(|p| u16::from_le_bytes(*p))
            .collect();
        search(&hay, &pin, start / 2, forward).map(|i| i * 2)
    } else {
        search(haystack, needle, start, forward)
    };
    found.map_or(-1.0, |i| i as f64)
}

/// `buffer_index_of_string(buf, needle, byteOffset, encoding, forward)`.
pub fn buffer_index_of_string(scope: &mut v8::PinScope, args: Args, mut rv: Ret) {
    let Some(haystack) = view_bytes(scope, &args, 0) else {
        return;
    };
    let Ok(needle) = v8::Local::<v8::String>::try_from(args.get(1)) else {
        return type_error(scope, "needle must be a string");
    };
    let offset = args.get(2).integer_value(scope).unwrap_or(0);
    let encoding = Encoding::from_value(scope, args.get(3));
    let forward = args.get(4).boolean_value(scope);
    let needle = encode(scope, needle, encoding);
    rv.set_double(index_of_bytes(haystack, &needle, offset, encoding, forward));
}

/// `buffer_index_of_buffer(buf, needle, byteOffset, encoding, forward)`.
pub fn buffer_index_of_buffer(scope: &mut v8::PinScope, args: Args, mut rv: Ret) {
    let (Some(haystack), Some(needle)) = (view_bytes(scope, &args, 0), view_bytes(scope, &args, 1))
    else {
        return;
    };
    let needle = needle.to_vec();
    let offset = args.get(2).integer_value(scope).unwrap_or(0);
    let encoding = Encoding::from_value(scope, args.get(3));
    let forward = args.get(4).boolean_value(scope);
    rv.set_double(index_of_bytes(haystack, &needle, offset, encoding, forward));
}

/// `buffer_index_of_number(buf, byte, byteOffset, forward)`.
pub fn buffer_index_of_number(scope: &mut v8::PinScope, args: Args, mut rv: Ret) {
    let Some(haystack) = view_bytes(scope, &args, 0) else {
        return;
    };
    let needle = args.get(1).uint32_value(scope).unwrap_or(0) as u8;
    let offset = args.get(2).integer_value(scope).unwrap_or(0);
    let forward = args.get(3).boolean_value(scope);
    let start = index_of_offset(haystack.len(), offset, 1, forward);
    if start <= -1 || haystack.is_empty() {
        return rv.set_int32(-1);
    }
    let start = start as usize;
    let found = if forward {
        haystack[start..]
            .iter()
            .position(|&b| b == needle)
            .map(|i| i + start)
    } else {
        haystack[..=start].iter().rposition(|&b| b == needle)
    };
    rv.set_double(found.map_or(-1.0, |i| i as f64));
}

fn swap(scope: &mut v8::PinScope, args: &Args, width: usize) {
    if let Some(data) = view_bytes(scope, args, 0) {
        for chunk in data.chunks_exact_mut(width) {
            chunk.reverse();
        }
    }
}

pub fn buffer_swap16(scope: &mut v8::PinScope, args: Args, _: Ret) {
    swap(scope, &args, 2);
}

pub fn buffer_swap32(scope: &mut v8::PinScope, args: Args, _: Ret) {
    swap(scope, &args, 4);
}

pub fn buffer_swap64(scope: &mut v8::PinScope, args: Args, _: Ret) {
    swap(scope, &args, 8);
}

/// `buffer_btoa(string)`: base64 of the string's Latin-1 bytes, or -1 if a
/// character is above U+00FF.
pub fn buffer_btoa(scope: &mut v8::PinScope, args: Args, mut rv: Ret) {
    let Ok(text) = v8::Local::<v8::String>::try_from(args.get(0)) else {
        return type_error(scope, "argument must be a string");
    };
    let units = utf16_units(scope, text);
    if units.iter().any(|&u| u > 0xFF) {
        return rv.set_int32(-1);
    }
    let bytes: Vec<u8> = units.iter().map(|&u| u as u8).collect();
    let encoded = base64_encode(&bytes, BASE64, true);
    if let Some(text) = v8::String::new_from_one_byte(scope, &encoded, v8::NewStringType::Normal) {
        rv.set(text.into());
    }
}

/// `buffer_atob(string)`: WHATWG forgiving-base64 decode, as a Latin-1
/// string; -2 for a character outside the alphabet, -1 when one character
/// is left over (the codes simdutf's errors map to in lib/buffer.js).
pub fn buffer_atob(scope: &mut v8::PinScope, args: Args, mut rv: Ret) {
    let Ok(text) = v8::Local::<v8::String>::try_from(args.get(0)) else {
        return type_error(scope, "argument must be a string");
    };
    let mut units: Vec<u16> = utf16_units(scope, text)
        .into_iter()
        .filter(|&u| !matches!(u, 0x09 | 0x0A | 0x0C | 0x0D | 0x20))
        .collect();
    if units.len().is_multiple_of(4) {
        for _ in 0..2 {
            if units.last() == Some(&u16::from(b'=')) {
                units.pop();
            }
        }
    }
    let valid = |u: u16| {
        u < 128 && matches!(u as u8, b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'+' | b'/')
    };
    if !units.iter().all(|&u| valid(u)) {
        return rv.set_int32(-2);
    }
    if units.len() % 4 == 1 {
        return rv.set_int32(-1);
    }
    let mut out = vec![0; base64_decoded_size_fast(units.len())];
    let written = base64_decode(&mut out, &units);
    out.truncate(written);
    if let Some(text) = v8::String::new_from_one_byte(scope, &out, v8::NewStringType::Normal) {
        rv.set(text.into());
    }
}
