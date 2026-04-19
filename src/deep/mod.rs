//! Deep-log recording: full-fidelity per-call capture to `.dclog` files.
//!
//! Off by default. Activated by setting `TRACEHASH_DEEP_DIR=<dir>` at runtime.
//! The `deep` cargo feature must also be enabled at build time.

pub mod reader;
pub mod replay;
pub mod sampling;
pub mod writer;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use sampling::SamplePolicy;
use writer::{make_header, sanitize_filename, DeepLog, DEFAULT_COMPRESSION_LEVEL};

use crate::spec::value::{Outcome, Value};

pub use reader::{LogEntry, LogReader};
pub use replay::{replay, replay_assert, replay_dir, EntryView, ReplayOutcome, ReplayReport};
pub use sampling::{Sample, SamplePolicyParseError, Sampler};

/// Runtime configuration resolved on first use.
struct DeepConfig {
    dir: PathBuf,
    default_policy: SamplePolicy,
    seed: u64,
    compression: i32,
    only: Option<HashSet<String>>,
}

impl DeepConfig {
    fn resolve() -> Option<Self> {
        let dir = match std::env::var("TRACEHASH_DEEP_DIR") {
            Ok(d) if !d.is_empty() => PathBuf::from(d),
            _ => return None,
        };
        std::fs::create_dir_all(&dir).ok()?;

        let default_policy = std::env::var("TRACEHASH_DEEP_MODE")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(SamplePolicy::FirstN(100));

        let seed = std::env::var("TRACEHASH_DEEP_SEED")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);

        let compression = std::env::var("TRACEHASH_COMPRESS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0)
            .min(22);

        let only = std::env::var("TRACEHASH_DEEP_ONLY")
            .ok()
            .filter(|s| !s.is_empty())
            .map(|s| s.split(',').map(str::to_string).collect::<HashSet<_>>());

        Some(Self {
            dir,
            default_policy,
            seed,
            compression,
            only,
        })
    }
}

/// Global state: config (resolved once) + per-function log file registry.
struct DeepState {
    config: DeepConfig,
    logs: Mutex<HashMap<String, Arc<Mutex<DeepLog>>>>,
}

static STATE: OnceLock<Option<DeepState>> = OnceLock::new();

fn state() -> Option<&'static DeepState> {
    STATE
        .get_or_init(|| {
            DeepConfig::resolve().map(|config| DeepState {
                config,
                logs: Mutex::new(HashMap::new()),
            })
        })
        .as_ref()
}

/// Is deep-mode capture active?
pub fn enabled() -> bool {
    state().is_some()
}

/// Is this function allowed by the `TRACEHASH_DEEP_ONLY` filter?
fn function_allowed(state: &DeepState, function: &str) -> bool {
    match &state.config.only {
        Some(set) => set.contains(function),
        None => true,
    }
}

fn open_log(state: &DeepState, function: &str) -> Option<Arc<Mutex<DeepLog>>> {
    let mut logs = state.logs.lock().ok()?;
    if let Some(log) = logs.get(function) {
        return Some(log.clone());
    }
    let filename = format!("{}.dclog", sanitize_filename(function));
    let path = state.config.dir.join(filename);
    let header = make_header(function, &state.config.default_policy, state.config.seed);
    let level = if state.config.compression > 0 {
        state.config.compression
    } else if state.config.compression == 0 {
        0
    } else {
        DEFAULT_COMPRESSION_LEVEL
    };
    let log = DeepLog::create(
        path,
        header,
        state.config.default_policy.clone(),
        state.config.seed,
        level,
    )
    .ok()?;
    let handle = Arc::new(Mutex::new(log));
    logs.insert(function.to_string(), handle.clone());
    Some(handle)
}

/// Record one call. Returns `Some(seq)` if an entry was written, which is
/// the `deep_seq` number that indexes into the per-function `.dclog` file.
pub(crate) fn record(
    function: &str,
    inputs: Vec<(String, Value)>,
    outputs: Vec<(String, Value)>,
) -> Option<u32> {
    let state = state()?;
    if !function_allowed(state, function) {
        return None;
    }
    let log = open_log(state, function)?;
    let mut guard = log.lock().ok()?;
    guard
        .record(None, None, inputs, Outcome::Return(outputs))
        .ok()
        .flatten()
}

/// Flush all buffered "last" entries and close writers. Call before exit
/// when using `firstlast:` or `prob:` with `keep_last`.
pub fn flush_all() {
    if let Some(state) = state() {
        if let Ok(mut logs) = state.logs.lock() {
            for (_name, log) in logs.iter_mut() {
                if let Ok(mut guard) = log.lock() {
                    let _ = guard.flush_last();
                }
            }
        }
    }
}
