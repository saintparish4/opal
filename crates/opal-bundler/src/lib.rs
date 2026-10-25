//! The bundler: one ES module that behaves like the unbundled program.
//!
//! What "behaves like" means, and what is refused instead, is the contract in
//! `SEMANTICS.md` next to this crate's manifest. Nothing here resolves an
//! import: the module graph comes from opal-core.

#[cfg(feature = "oracle")]
pub mod oracle;
