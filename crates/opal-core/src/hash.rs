//! BLAKE3 content hashing.
//!
//! Every identity in Opal — CAS keys, memo keys, "did this change?" — is a
//! [`ContentHash`] over bytes. Nothing in this module, or anything built on it,
//! looks at a timestamp: mtimes are unreliable across git checkouts, CI runners,
//! and Docker layers (PRD §4.2), so they are not an input to any decision Opal
//! makes.

use std::fmt;
use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

pub const HASH_BYTES: usize = 32;
pub const HASH_HEX_LEN: usize = HASH_BYTES * 2;

/// A BLAKE3 digest of some content
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ContentHash([u8; HASH_BYTES]);

impl ContentHash {
    /// Hashes a byte slice
    pub fn of(bytes: &[u8]) -> Self {
        Self(*blake3::hash(bytes).as_bytes())
    }

    /// Hashes a stream without holding it in memory
    pub fn of_reader(reader: impl Read) -> io::Result<Self> {
        let mut hasher = blake3::Hasher::new();
        hasher.update_reader(reader)?;
        Ok(Self(*hasher.finalize().as_bytes()))
    }

    /// Hashes a file's content
    pub fn of_file(path: &Path) -> io::Result<Self> {
        Self::of_reader(File::open(path)?)
    }

    pub fn as_bytes(&self) -> &[u8; HASH_BYTES] {
        &self.0
    }

    pub fn from_bytes(bytes: [u8; HASH_BYTES]) -> Self {
        Self(bytes)
    }

    /// Lowercase hex, 64 characters. This is the on-disk spelling everywhere:
    /// CAS filenames, memo filenames, JSON output.
    pub fn to_hex(&self) -> String {
        const DIGITS: &[u8; 16] = b"0123456789abcdef";
        let mut out = String::with_capacity(HASH_HEX_LEN);
        for byte in self.0 {
            out.push(DIGITS[usize::from(byte >> 4)] as char);
            out.push(DIGITS[usize::from(byte & 0x0f)] as char);
        }
        out
    }

    pub fn parse_hex(text: &str) -> Result<Self, HashParseError> {
        if text.len() != HASH_HEX_LEN {
            return Err(HashParseError::Length(text.len()));
        }
        let mut bytes = [0u8; HASH_BYTES];
        for (index, byte) in bytes.iter_mut().enumerate() {
            let pair = &text[index * 2..index * 2 + 2];
            *byte = u8::from_str_radix(pair, 16).map_err(|_| HashParseError::Digit)?;
        }
        Ok(Self(bytes))
    }
}

/// Incremental hasher for content that arrives in chunks.
///
/// Unlike [`HashBuilder`] this adds no framing: the digest equals
/// [`ContentHash::of`] over the concatenated chunks, which is what makes it
/// usable for hashing a file while it streams past on its way into the CAS.
#[derive(Default)]
pub struct ContentHasher {
    hasher: blake3::Hasher,
}

impl ContentHasher {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn update(&mut self, bytes: &[u8]) -> &mut Self {
        self.hasher.update(bytes);
        self
    }

    pub fn finish(&self) -> ContentHash {
        ContentHash(*self.hasher.finalize().as_bytes())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum HashParseError {
    #[error("expected {HASH_HEX_LEN} hex characters, got {0}")]
    Length(usize),
    #[error("value is not lowercase hex")]
    Digit,
}

/// Builds a hash out of several parts — the primitive behind every derived key
/// (memo keys, graph digests).
///
/// Parts are length-prefixed, so `("ab", "c")` and `("a", "bc")` cannot produce
/// the same key, and every builder starts from a domain string, so a key derived
/// for one purpose can never collide with a key derived for another.
pub struct HashBuilder {
    hasher: blake3::Hasher,
}

impl HashBuilder {
    pub fn new(domain: &str) -> Self {
        let mut builder = Self {
            hasher: blake3::Hasher::new(),
        };
        builder.push_bytes(domain.as_bytes());
        builder
    }

    pub fn push_bytes(&mut self, bytes: &[u8]) -> &mut Self {
        self.hasher.update(&(bytes.len() as u64).to_le_bytes());
        self.hasher.update(bytes);
        self
    }

    pub fn push_str(&mut self, text: &str) -> &mut Self {
        self.push_bytes(text.as_bytes())
    }

    pub fn push_u64(&mut self, value: u64) -> &mut Self {
        self.push_bytes(&value.to_le_bytes())
    }

    pub fn push_hash(&mut self, hash: &ContentHash) -> &mut Self {
        self.push_bytes(hash.as_bytes())
    }

    pub fn finish(&self) -> ContentHash {
        ContentHash(*self.hasher.finalize().as_bytes())
    }
}

impl fmt::Display for ContentHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

impl fmt::Debug for ContentHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Full hex, not a prefix: a truncated hash in a failure message is the
        // one thing you cannot paste back into `opal cache` to investigate.
        write!(f, "ContentHash({})", self.to_hex())
    }
}

impl Serialize for ContentHash {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for ContentHash {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse_hex(&text).map_err(D::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use std::fs::{self, FileTimes};
    use std::io::Write as _;
    use std::time::{Duration, SystemTime};

    use super::*;

    #[test]
    fn test_same_input_same_hash() {
        assert_eq!(
            ContentHash::of(b"export default 1"),
            ContentHash::of(b"export default 1")
        );
        assert_ne!(ContentHash::of(b"a"), ContentHash::of(b"b"));
        assert_eq!(ContentHash::of(b"").to_hex().len(), HASH_HEX_LEN);
    }

    #[test]
    fn test_hex_round_trip() {
        let hash = ContentHash::of(b"round trip");
        assert_eq!(ContentHash::parse_hex(&hash.to_hex()).unwrap(), hash);
        assert!(ContentHash::parse_hex("abc").is_err());
        assert!(ContentHash::parse_hex(&"z".repeat(HASH_HEX_LEN)).is_err());
    }

    #[test]
    fn test_streaming_matches_in_memory() {
        // Larger than BLAKE3's internal chunk size, so the streaming path
        // actually spans multiple chunks.
        let data = vec![7u8; 100_000];
        assert_eq!(
            ContentHash::of_reader(data.as_slice()).unwrap(),
            ContentHash::of(&data)
        );
    }

    #[test]
    fn test_hash_ignores_mtime() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.js");
        let mut file = File::create(&path).unwrap();
        file.write_all(b"export const a = 1;").unwrap();
        file.sync_all().unwrap();

        let before = ContentHash::of_file(&path).unwrap();

        let times = FileTimes::new()
            .set_modified(SystemTime::now() + Duration::from_secs(3600))
            .set_accessed(SystemTime::now() + Duration::from_secs(3600));
        File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_times(times)
            .unwrap();

        assert_ne!(
            fs::metadata(&path).unwrap().modified().unwrap(),
            SystemTime::UNIX_EPOCH
        );
        assert_eq!(ContentHash::of_file(&path).unwrap(), before);
    }

    #[test]
    fn test_chunked_hashing_matches_whole() {
        let mut hasher = ContentHasher::new();
        hasher.update(b"import a ").update(b"from './a.js';");
        assert_eq!(hasher.finish(), ContentHash::of(b"import a from './a.js';"));
    }

    #[test]
    fn test_builder_is_domain_separated() {
        let one = HashBuilder::new("opal.graph.v1").push_str("a").finish();
        let two = HashBuilder::new("opal.memo.v1").push_str("a").finish();
        assert_ne!(one, two);
    }

    #[test]
    fn test_builder_parts_are_unambiguous() {
        let split_one = HashBuilder::new("d").push_str("ab").push_str("c").finish();
        let split_two = HashBuilder::new("d").push_str("a").push_str("bc").finish();
        assert_ne!(split_one, split_two);
    }
}
