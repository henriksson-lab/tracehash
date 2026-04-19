//! Per-function `.dclog` writer. One file per function name, opened lazily.
//!
//! Matches deep-comparator's `.dclog` format so its tooling and existing
//! `.dclog` consumers can read tracehash output.

use std::collections::HashSet;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;

use super::sampling::{Sample, SamplePolicy, Sampler};
use crate::spec::header::{LogHeader, RecorderConfig, SourceLang};
use crate::spec::value::{Outcome, Value};
use crate::spec::wire::{canonical_input_bytes, write_entry_body, write_file_prefix, Entry};
use crate::spec::Result;

pub const DEFAULT_COMPRESSION_LEVEL: i32 = 3;

/// FNV-1a 64-bit content hash (same as `stable_hash::Fnv64`, unrolled for
/// the dedup fast path on canonicalized input bytes).
fn content_hash(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

pub struct DeepLog {
    inner: Box<dyn Write + Send>,
    sampler: Sampler,
    seq_counter: u64,
    last_buffer: Option<Vec<u8>>,
    seen_hashes: HashSet<u64>,
}

impl DeepLog {
    pub fn create(
        path: PathBuf,
        header: LogHeader,
        policy: SamplePolicy,
        seed: u64,
        compression_level: i32,
    ) -> Result<Self> {
        let f = File::create(path)?;
        let buffered = BufWriter::new(f);

        let mut inner: Box<dyn Write + Send> = if compression_level > 0 {
            let enc = zstd::stream::Encoder::new(buffered, compression_level)?;
            Box::new(enc.auto_finish())
        } else {
            Box::new(buffered)
        };

        write_file_prefix(&mut inner, &header)?;

        Ok(Self {
            inner,
            sampler: Sampler::new(policy, seed),
            seq_counter: 0,
            last_buffer: None,
            seen_hashes: HashSet::new(),
        })
    }

    /// Record one call. On success, returns `Some(seq)` if the entry was
    /// written to disk (the `seq` value that appears in the matching TSV
    /// row's `deep_seq` column), or `None` if sampled out / deduped.
    pub fn record(
        &mut self,
        receiver_in: Option<Value>,
        receiver_out: Option<Value>,
        inputs: Vec<(String, Value)>,
        outcome: Outcome,
    ) -> Result<Option<u32>> {
        let seq = self.seq_counter;
        self.seq_counter += 1;

        let decision = self.sampler.decide(seq);
        if decision == Sample::Skip {
            return Ok(None);
        }

        let canon = canonical_input_bytes(receiver_in.as_ref(), &inputs)?;
        let hash = content_hash(&canon);

        let entry = Entry {
            seq: seq as u32,
            receiver_in,
            receiver_out,
            inputs,
            outcome,
            content_hash: hash,
        };

        match decision {
            Sample::Record => {
                if !self.seen_hashes.insert(hash) {
                    return Ok(None);
                }
                let mut buf = Vec::with_capacity(128);
                write_entry_body(&mut buf, &entry)?;
                self.write_framed(&buf)?;
                Ok(Some(seq as u32))
            }
            Sample::BufferAsLast => {
                let mut buf = Vec::with_capacity(128);
                write_entry_body(&mut buf, &entry)?;
                self.last_buffer = Some(buf);
                Ok(None)
            }
            Sample::Skip => unreachable!(),
        }
    }

    fn write_framed(&mut self, body: &[u8]) -> Result<()> {
        self.inner.write_all(&(body.len() as u32).to_le_bytes())?;
        self.inner.write_all(body)?;
        Ok(())
    }

    pub fn flush_last(&mut self) -> Result<()> {
        if let Some(buf) = self.last_buffer.take() {
            self.write_framed(&buf)?;
        }
        self.inner.flush()?;
        Ok(())
    }
}

/// Build a `LogHeader` for a per-function dclog that tracehash emits.
pub fn make_header(function_name: &str, policy: &SamplePolicy, seed: u64) -> LogHeader {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    LogHeader {
        spec_version: crate::spec::SPEC_VERSION,
        source_lang: SourceLang::Rust,
        function_name: function_name.to_string(),
        function_display: function_name.to_string(),
        signature_fingerprint: String::new(),
        timestamp,
        recorder_config: RecorderConfig {
            mode: policy.to_string(),
            seed,
            extra: serde_json::Value::Null,
        },
        schemas: Vec::new(),
    }
}

/// Replace path-unfriendly characters in a function name for use as a
/// filename stem. Keeps alphanumerics, `_`, `-`, `.` and `::`; everything
/// else becomes `_`.
pub fn sanitize_filename(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' || ch == '.' || ch == ':' {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    out
}
