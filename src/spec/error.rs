use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("unexpected end of stream")]
    UnexpectedEof,

    #[error("bad magic: expected DCLG, got {0:02x?}")]
    BadMagic([u8; 4]),

    #[error("unsupported spec version {0} (this build supports {1})")]
    UnsupportedVersion(u32, u32),

    #[error("unknown tag byte 0x{0:02x}")]
    UnknownTag(u8),

    #[error("invalid utf-8 in string field")]
    InvalidUtf8,

    #[error("value-shape mismatch: expected {expected}, got {actual}")]
    ShapeMismatch {
        expected: &'static str,
        actual: String,
    },

    #[error("header parse error: {0}")]
    HeaderParse(#[from] serde_json::Error),

    #[error("field missing: {0}")]
    MissingField(String),

    #[error("duplicate id {0} in shared-id map")]
    DuplicateId(u32),

    #[error("unknown shared id {0}; log stream invariant violated")]
    UnknownId(u32),

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;
