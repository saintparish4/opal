//! Subresource-integrity strings, as the npm registry publishes them.
//!
//! `dist.integrity` is `sha512-<base64>`; packages published before 2017 carry
//! only `dist.shasum`, a hex sha1. Both are verified against the tarball bytes
//! *before* anything derived from them enters the CAS, because BLAKE3 addressing
//! answers "are these the bytes I stored" — not "are these the bytes npm
//! published".

use std::fmt;

use sha1::Sha1;
use sha2::{Digest, Sha512};

#[derive(Debug, thiserror::Error)]
pub enum IntegrityError {
    #[error("{0:?} is not a supported integrity string")]
    Unsupported(String),
    #[error("integrity mismatch: expected {expected}, got {actual}")]
    Mismatch { expected: String, actual: String },
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Algorithm {
    Sha512,
    Sha1,
}

impl Algorithm {
    fn name(self) -> &'static str {
        match self {
            Self::Sha512 => "sha512",
            Self::Sha1 => "sha1",
        }
    }

    fn digest(self, bytes: &[u8]) -> Vec<u8> {
        match self {
            Self::Sha512 => Sha512::digest(bytes).to_vec(),
            Self::Sha1 => Sha1::digest(bytes).to_vec(),
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Integrity {
    algorithm: Algorithm,
    digest: Vec<u8>,
}

impl Integrity {
    pub fn of(algorithm: Algorithm, bytes: &[u8]) -> Self {
        Self {
            algorithm,
            digest: algorithm.digest(bytes),
        }
    }

    /// Parses one entry, or the strongest of several space-separated entries
    pub fn parse(text: &str) -> Result<Self, IntegrityError> {
        let unsupported = || IntegrityError::Unsupported(text.to_string());
        let mut best: Option<Self> = None;
        for entry in text.split_whitespace() {
            let Some((algorithm, encoded)) = entry.split_once('-') else {
                continue;
            };
            let algorithm = match algorithm {
                "sha512" => Algorithm::Sha512,
                "sha1" => Algorithm::Sha1,
                // sha256 and friends are legal SRI but npm does not publish
                // them; skipping beats pretending to verify.
                _ => continue,
            };
            let digest = base64_decode(encoded).ok_or_else(unsupported)?;
            let candidate = Self { algorithm, digest };
            if algorithm == Algorithm::Sha512 {
                return Ok(candidate);
            }
            best.get_or_insert(candidate);
        }
        best.ok_or_else(unsupported)
    }

    /// Builds an integrity from a legacy hex `dist.shasum`.
    pub fn from_shasum(hex: &str) -> Result<Self, IntegrityError> {
        let unsupported = || IntegrityError::Unsupported(hex.to_string());
        if hex.len() != 40 {
            return Err(unsupported());
        }
        let digest = (0..20)
            .map(|index| u8::from_str_radix(&hex[index * 2..index * 2 + 2], 16).ok())
            .collect::<Option<Vec<u8>>>()
            .ok_or_else(unsupported)?;
        Ok(Self {
            algorithm: Algorithm::Sha1,
            digest,
        })
    }

    pub fn verify(&self, bytes: &[u8]) -> Result<(), IntegrityError> {
        let actual = Self::of(self.algorithm, bytes);
        if actual == *self {
            Ok(())
        } else {
            Err(IntegrityError::Mismatch {
                expected: self.to_string(),
                actual: actual.to_string(),
            })
        }
    }
}

impl fmt::Display for Integrity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}-{}",
            self.algorithm.name(),
            base64_encode(&self.digest)
        )
    }
}

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let mut buffer = [0u8; 3];
        buffer[..chunk.len()].copy_from_slice(chunk);
        let packed = u32::from(buffer[0]) << 16 | u32::from(buffer[1]) << 8 | u32::from(buffer[2]);
        for index in 0..4 {
            if index <= chunk.len() {
                let shift = 18 - index * 6;
                out.push(ALPHABET[(packed >> shift) as usize & 0x3f] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let mut packed: u32 = 0;
    let mut bits = 0;
    for byte in text.bytes() {
        if byte == b'=' {
            break;
        }
        let value = ALPHABET.iter().position(|candidate| *candidate == byte)? as u32;
        packed = packed << 6 | value;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((packed >> bits) as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_base64_round_trips() {
        for payload in [&b""[..], b"f", b"fo", b"foo", b"foob", b"fooba", b"foobar"] {
            let encoded = base64_encode(payload);
            assert_eq!(base64_decode(&encoded).unwrap(), payload, "{encoded}");
        }
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
    }

    #[test]
    fn test_verifies_matching_bytes() {
        let integrity = Integrity::of(Algorithm::Sha512, b"tarball bytes");
        integrity.verify(b"tarball bytes").unwrap();
        assert!(matches!(
            integrity.verify(b"other bytes"),
            Err(IntegrityError::Mismatch { .. })
        ));
    }

    #[test]
    fn test_round_trips_through_its_string_form() {
        let integrity = Integrity::of(Algorithm::Sha512, b"payload");
        let text = integrity.to_string();
        assert!(text.starts_with("sha512-"));
        assert_eq!(Integrity::parse(&text).unwrap(), integrity);
    }

    #[test]
    fn test_prefers_sha512_when_several_are_offered() {
        let strong = Integrity::of(Algorithm::Sha512, b"payload");
        let weak = Integrity::of(Algorithm::Sha1, b"payload");
        let combined = format!("{weak} {strong}");
        assert_eq!(Integrity::parse(&combined).unwrap(), strong);
    }

    #[test]
    fn test_legacy_shasum() {
        let expected = Integrity::of(Algorithm::Sha1, b"payload");
        let hex: String = expected
            .digest
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        assert_eq!(Integrity::from_shasum(&hex).unwrap(), expected);
        assert!(Integrity::from_shasum("nope").is_err());
    }

    #[test]
    fn test_rejects_unknown_algorithms() {
        assert!(Integrity::parse("md5-abc").is_err());
        assert!(Integrity::parse("").is_err());
    }
}
