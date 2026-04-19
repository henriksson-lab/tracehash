//! Canonical dclog wire format.
//!
//! Byte-for-byte compatible with deep-comparator's `.dclog` format
//! (MAGIC="DCLG", SPEC_VERSION=1). This is the on-disk contract every
//! recorder (C, C++, future JVM) and the replay reader must agree on.

pub mod error;
pub mod header;
pub mod tags;
pub mod value;
pub mod wire;

pub use error::{Error, Result};
pub use header::{FieldKind, LogHeader, RecorderConfig, Schema, SchemaField, SourceLang};
pub use value::{Outcome, Value};

pub const MAGIC: [u8; 4] = *b"DCLG";
pub const SPEC_VERSION: u32 = 1;
