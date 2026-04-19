//! C-side dclog parity: run a C probe against a temp directory, read back
//! the `.dclog` with the Rust reader, and compare it to the entry the Rust
//! side would produce for the same probe.

#![cfg(feature = "deep")]

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use tracehash::spec::Value;

#[test]
fn c_probe_named_fields_roundtrip_via_rust_reader() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let dir = std::env::temp_dir().join(format!(
        "tracehash-c-deep-rt-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
    ));
    fs::create_dir_all(&dir).unwrap();
    let deep_dir = dir.join("deep");
    fs::create_dir_all(&deep_dir).unwrap();
    let c_path = dir.join("probe.c");
    let exe_path = dir.join("probe");

    fs::write(
        &c_path,
        r#"
#include "tracehash_c.h"

int main(void) {
  TH_CALL("c_probe");
  TH_IN_U64_AS("seq_len", 116);
  TH_IN_F32_AS("score", 12.5f);
  TH_OUT_BOOL_AS("passed", 1);
  TH_OUT_I64_AS("delta", -3);
  TH_FINISH();
  tracehash_deep_flush_all();
  return 0;
}
"#,
    )
    .unwrap();

    let cc = std::env::var("CC").unwrap_or_else(|_| "cc".to_string());
    let status = Command::new(&cc)
        .arg("-std=c99")
        .arg("-I")
        .arg(manifest.join("c"))
        .arg(&c_path)
        .arg(manifest.join("c/tracehash_c.c"))
        .arg("-o")
        .arg(&exe_path)
        .arg("-lpthread")
        .arg("-lm")
        .status()
        .unwrap();
    assert!(status.success());

    let status = Command::new(&exe_path)
        .env("TRACEHASH_DEEP_DIR", &deep_dir)
        .env("TRACEHASH_DEEP_MODE", "all")
        .status()
        .unwrap();
    assert!(status.success());

    let dclog = deep_dir.join("c_probe.dclog");
    assert!(dclog.exists(), "C probe should produce {:?}", dclog);

    let reader = tracehash::deep::LogReader::open(&dclog).unwrap();
    assert_eq!(reader.header().function_name, "c_probe");
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
            assert_eq!(outputs.len(), 2);
            assert_eq!(outputs[0].0, "passed");
            assert_eq!(outputs[0].1, Value::Bool(true));
            assert_eq!(outputs[1].0, "delta");
            assert_eq!(outputs[1].1, Value::I64(-3));
        }
        _ => panic!("expected Return outcome"),
    }
}

#[test]
fn c_probe_positional_uses_auto_names() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let dir = std::env::temp_dir().join(format!(
        "tracehash-c-deep-auto-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
    ));
    fs::create_dir_all(&dir).unwrap();
    let deep_dir = dir.join("deep");
    fs::create_dir_all(&deep_dir).unwrap();
    let c_path = dir.join("probe.c");
    let exe_path = dir.join("probe");

    fs::write(
        &c_path,
        r#"
#include "tracehash_c.h"

int main(void) {
  TH_CALL("auto_probe");
  TH_IN_U64(42);
  TH_IN_F32(3.5f);
  TH_OUT_U64(7);
  TH_FINISH();
  tracehash_deep_flush_all();
  return 0;
}
"#,
    )
    .unwrap();

    let cc = std::env::var("CC").unwrap_or_else(|_| "cc".to_string());
    let status = Command::new(&cc)
        .arg("-std=c99")
        .arg("-I")
        .arg(manifest.join("c"))
        .arg(&c_path)
        .arg(manifest.join("c/tracehash_c.c"))
        .arg("-o")
        .arg(&exe_path)
        .arg("-lpthread")
        .arg("-lm")
        .status()
        .unwrap();
    assert!(status.success());

    let status = Command::new(&exe_path)
        .env("TRACEHASH_DEEP_DIR", &deep_dir)
        .env("TRACEHASH_DEEP_MODE", "all")
        .status()
        .unwrap();
    assert!(status.success());

    let dclog = deep_dir.join("auto_probe.dclog");
    let reader = tracehash::deep::LogReader::open(&dclog).unwrap();
    let entries = reader.collect_entries().unwrap();
    assert_eq!(entries.len(), 1);
    let e = &entries[0];
    assert_eq!(e.inputs[0].0, "in0");
    assert_eq!(e.inputs[0].1, Value::U64(42));
    assert_eq!(e.inputs[1].0, "in1");
    assert_eq!(e.inputs[1].1, Value::F32(3.5));
    match &e.outcome {
        tracehash::spec::Outcome::Return(outputs) => {
            assert_eq!(outputs[0].0, "out0");
            assert_eq!(outputs[0].1, Value::U64(7));
        }
        _ => panic!(),
    }
}
