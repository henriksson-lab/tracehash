//! Log header: JSON metadata describing the recording.
//!
//! Lives between the file magic/version prefix and the entry stream. Emitted
//! as JSON so the C writer can produce it trivially.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SourceLang {
    C,
    Cpp,
    Rust,
    Jvm,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogHeader {
    pub spec_version: u32,
    pub source_lang: SourceLang,
    pub function_name: String,
    pub function_display: String,
    pub signature_fingerprint: String,
    pub timestamp: i64,
    pub recorder_config: RecorderConfig,
    pub schemas: Vec<Schema>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecorderConfig {
    pub mode: String,
    #[serde(default)]
    pub seed: u64,
    #[serde(default)]
    pub extra: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Schema {
    pub id: u32,
    pub name: String,
    pub fields: Vec<SchemaField>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaField {
    pub name: String,
    pub r#type: String,
    #[serde(default)]
    pub kind: FieldKind,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum FieldKind {
    #[default]
    Value,
    Shared,
    Weak,
    Unique,
}
