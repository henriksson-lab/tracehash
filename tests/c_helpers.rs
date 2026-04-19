use std::fs;
use std::process::Command;
use tracehash::TraceHash;

#[derive(TraceHash)]
struct PipelineDecision {
    seq_len: usize,
    model_len: usize,
    score: f32,
    passed: bool,
}

#[test]
fn c_struct_helpers_match_rust_derive_hashing() {
    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let dir = std::env::temp_dir().join(format!("tracehash-c-test-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let c_path = dir.join("probe.c");
    let exe_path = dir.join("probe");
    let trace_path = dir.join("trace.tsv");

    fs::write(
        &c_path,
        r#"
#include "tracehash_c.h"

typedef struct PipelineDecision {
  uint64_t seq_len;
  uint64_t model_len;
  float score;
  int passed;
} PipelineDecision;

#define PIPELINE_DECISION_FIELDS(X, call, value) \
  X##_U64(call, value, seq_len) \
  X##_U64(call, value, model_len) \
  X##_F32(call, value, score) \
  X##_BOOL(call, value, passed)

TH_DEFINE_STRUCT_HASH(PipelineDecision, PIPELINE_DECISION_FIELDS)

int main(void) {
  PipelineDecision value;
  value.seq_len = 116;
  value.model_len = 262;
  value.score = 12.5f;
  value.passed = 1;

  TH_CALL("derive_parity");
  TH_OUT_STRUCT(PipelineDecision, &value);
  TH_FINISH();
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
        .unwrap_or_else(|err| panic!("failed to execute {cc}: {err}"));
    assert!(status.success(), "{cc} failed to build C helper probe");

    let status = Command::new(&exe_path)
        .env("TRACEHASH_OUT", &trace_path)
        .env("TRACEHASH_SIDE", "c")
        .status()
        .unwrap();
    assert!(status.success(), "C helper probe failed");

    let trace = fs::read_to_string(&trace_path).unwrap();
    let row = trace.lines().next().expect("C trace row");
    let output_hash = row.split('\t').nth(6).expect("output hash column");

    let value = PipelineDecision {
        seq_len: 116,
        model_len: 262,
        score: 12.5,
        passed: true,
    };
    let mut expected = tracehash::Fnv64::new();
    expected.str("derive_parity");
    expected.u8(b'V');
    value.trace_hash(&mut expected);

    assert_eq!(output_hash, format!("{:016x}", expected.finish()));
}

#[test]
fn cpp_raii_header_builds_and_emits_trace() {
    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let dir = std::env::temp_dir().join(format!("tracehash-cpp-test-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let cpp_path = dir.join("probe.cpp");
    let exe_path = dir.join("probe_cpp");
    let trace_path = dir.join("trace.tsv");

    fs::write(
        &cpp_path,
        r#"
#include "tracehash_cpp.hpp"

int main() {
  TRACEHASH_CALL("cpp_probe");
  th_call.input_u64(7);
  th_call.output_bool(true);
  return 0;
}
"#,
    )
    .unwrap();

    let cxx = std::env::var("CXX").unwrap_or_else(|_| "c++".to_string());
    let status = Command::new(&cxx)
        .arg("-std=c++11")
        .arg("-I")
        .arg(manifest.join("c"))
        .arg(&cpp_path)
        .arg(manifest.join("c/tracehash_c.c"))
        .arg("-o")
        .arg(&exe_path)
        .arg("-lpthread")
        .arg("-lm")
        .status()
        .unwrap_or_else(|err| panic!("failed to execute {cxx}: {err}"));
    assert!(status.success(), "{cxx} failed to build C++ helper probe");

    let status = Command::new(&exe_path)
        .env("TRACEHASH_OUT", &trace_path)
        .env("TRACEHASH_SIDE", "cpp")
        .status()
        .unwrap();
    assert!(status.success(), "C++ helper probe failed");

    let trace = fs::read_to_string(&trace_path).unwrap();
    assert!(
        trace.contains("\tcpp\t"),
        "missing C++ side marker: {trace}"
    );
    assert!(
        trace.contains("\tcpp_probe\t"),
        "missing C++ function name: {trace}"
    );
}

#[test]
fn c_helpers_can_append_debug_values() {
    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let dir = std::env::temp_dir().join(format!("tracehash-c-values-test-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let c_path = dir.join("probe.c");
    let exe_path = dir.join("probe");
    let trace_path = dir.join("trace.tsv");

    fs::write(
        &c_path,
        r#"
#include "tracehash_c.h"

int main(void) {
  unsigned char payload[3] = {1, 2, 3};
  TH_CALL("value_probe");
  TH_IN_U64(42);
  TH_IN_BYTES(payload, 3);
  TH_OUT_F32(12.5f);
  TH_OUT_U64(7);
  TH_FINISH();
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
        .unwrap_or_else(|err| panic!("failed to execute {cc}: {err}"));
    assert!(status.success(), "{cc} failed to build C value probe");

    let status = Command::new(&exe_path)
        .env("TRACEHASH_OUT", &trace_path)
        .env("TRACEHASH_SIDE", "c")
        .env("TRACEHASH_VALUES", "1")
        .status()
        .unwrap();
    assert!(status.success(), "C value probe failed");

    let trace = fs::read_to_string(&trace_path).unwrap();
    let row = trace.lines().next().expect("C value trace row");
    let cols: Vec<_> = row.split('\t').collect();
    assert_eq!(cols.len(), 14, "expected debug value column: {row}");
    assert_eq!(cols[12], "-", "deep_seq should be '-' when deep mode off: {row}");
    assert!(cols[13].contains("IU64=42"), "missing input value: {row}");
    assert!(
        cols[13].contains("IBYTES=3:"),
        "missing byte summary: {row}"
    );
    assert!(
        cols[13].contains("OF32=41480000/"),
        "missing float value: {row}"
    );
    assert!(cols[13].contains("OU64=7"), "missing output value: {row}");
}

#[test]
fn c_named_field_helpers_match_rust_hashing() {
    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let dir = std::env::temp_dir().join(format!("tracehash-c-fields-test-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let c_path = dir.join("probe.c");
    let exe_path = dir.join("probe");
    let trace_path = dir.join("trace.tsv");

    fs::write(
        &c_path,
        r#"
#include "tracehash_c.h"

int main(void) {
  TH_CALL("field_probe");
  TH_IN_FIELD_U64("seq_len", 116);
  TH_IN_FIELD_F32("score", 12.5f);
  TH_OUT_FIELD_BOOL("passed", 1);
  TH_OUT_FIELD_I64("delta", -3);
  TH_FINISH();
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
        .unwrap_or_else(|err| panic!("failed to execute {cc}: {err}"));
    assert!(status.success(), "{cc} failed to build C field probe");

    let status = Command::new(&exe_path)
        .env("TRACEHASH_OUT", &trace_path)
        .env("TRACEHASH_SIDE", "c")
        .env("TRACEHASH_VALUES", "1")
        .status()
        .unwrap();
    assert!(status.success(), "C field probe failed");

    let trace = fs::read_to_string(&trace_path).unwrap();
    let row = trace.lines().next().expect("C field trace row");
    let cols: Vec<_> = row.split('\t').collect();

    let mut input = tracehash::Fnv64::new();
    input.str("field_probe");
    input.u8(b'G');
    input.str("seq_len");
    116u64.trace_hash(&mut input);
    input.u8(b'G');
    input.str("score");
    12.5f32.trace_hash(&mut input);

    let mut output = tracehash::Fnv64::new();
    output.str("field_probe");
    output.u8(b'G');
    output.str("passed");
    true.trace_hash(&mut output);
    output.u8(b'G');
    output.str("delta");
    (-3i64).trace_hash(&mut output);

    assert_eq!(cols[5], format!("{:016x}", input.finish()));
    assert_eq!(cols[6], format!("{:016x}", output.finish()));
    assert_eq!(cols[12], "-", "deep_seq should be '-' when deep mode off: {row}");
    assert!(
        cols[13].contains("IFIELD=seq_len"),
        "missing field value: {row}"
    );
    assert!(
        cols[13].contains("OFIELD=delta"),
        "missing field value: {row}"
    );
}
