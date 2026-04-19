//! End-to-end tests for the `deep` feature: verify that probes produce
//! readable `.dclog` entries, that `_as` named calls do not change the hash
//! stream, and that positional calls get auto-generated names.
//!
//! These tests mutate process-wide env vars and the global tracehash
//! writer, so they run serialized via a single module-scope Mutex.

#![cfg(feature = "deep")]

use std::path::PathBuf;
use std::sync::Mutex;

use tracehash::spec::Value;

static GUARD: Mutex<()> = Mutex::new(());

fn unique_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "tracehash-deep-{}-{}-{}",
        tag,
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn reset_env() {
    // These must be unset so other tests see a clean environment. The
    // WRITER OnceLock will still hold its first-resolved state across
    // tests in the same process, but the deep state resolves per-process
    // too. Tests that need independent state should run in separate
    // processes — which cargo test does by default when run as separate
    // test binaries.
    std::env::remove_var("TRACEHASH_OUT");
    std::env::remove_var("TRACEHASH_DEEP_DIR");
    std::env::remove_var("TRACEHASH_DEEP_MODE");
    std::env::remove_var("TRACEHASH_VALUES");
}

#[test]
fn deep_mode_records_named_primitives() {
    let _g = GUARD.lock().unwrap();
    reset_env();

    let dir = unique_dir("named");
    std::env::set_var("TRACEHASH_DEEP_DIR", dir.to_str().unwrap());
    std::env::set_var("TRACEHASH_DEEP_MODE", "all");

    {
        let mut th = tracehash::th_call!("probe_a");
        th.input_u64_as("seq_len", 116);
        th.input_f32_as("score", 12.5);
        th.output_bool_as("passed", true);
        th.finish();
    }
    tracehash::deep::flush_all();

    let path = dir.join("probe_a.dclog");
    assert!(path.exists(), "expected {:?} to be written", path);

    let reader = tracehash::deep::LogReader::open(&path).unwrap();
    assert_eq!(reader.header().function_name, "probe_a");
    let entries = reader.collect_entries().unwrap();
    assert_eq!(entries.len(), 1);
    let e = &entries[0];
    assert_eq!(e.inputs.len(), 2);
    assert_eq!(e.inputs[0].0, "seq_len");
    assert_eq!(e.inputs[0].1, Value::U64(116));
    assert_eq!(e.inputs[1].0, "score");
    assert_eq!(e.inputs[1].1, Value::F32(12.5));
    match &e.outcome {
        tracehash::spec::Outcome::Return(outputs) => {
            assert_eq!(outputs.len(), 1);
            assert_eq!(outputs[0].0, "passed");
            assert_eq!(outputs[0].1, Value::Bool(true));
        }
        _ => panic!("expected Return outcome"),
    }
}

