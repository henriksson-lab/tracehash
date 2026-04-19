# tracehash - support for faithful translation of code

`tracehash` is a small cross-language tracing toolkit for algorithm parity debugging. It records inputs and outputs of function calls
such that two implementations can be compared.

* In the first light pass, only hashes of inputs and outputs are stored, saving space and making comparison easy. If the output hash differs for a given input hash, this is a red flag
* In the second heavy pass, the full input and output data can be stored for problematic functions, enabling reasoning about what is going wrong

Previously a separate crate was used for deep comparison. But by merging them, the code need not be instrumented twice, making it easier to swap between the modes.

The code has been used to successfully find bugs. While Claude/Codex can be surprisingly good at reasoning, the LLM
is not yet at a point where it can be trusted for faithful translation. By comparing call frequency, and inputs vs outputs,
complicated bugs can be tracked down without complicated reasoning. This reduces cost and speeds up LLM-mediated translation.

This software assumes that translation is performed function-by-function (as far as possible), enabling 1-to-1 comparison.
It also assumes that functions are pure: one input gives one output. Other functions cannot be traced and writing such functions
should generally be considered poor practice (untestable code).

## How to use

Simply ask your LLM of choice to look at this Github repository and suggest to use it for tracking problems. This appears sufficient
to get it used.


## On the use of LLM and license

This code was generated using LLM, with the intent of being used by LLM. It might be useful for manual
testing but the focus is to aid faithful translation using LLM.

This code is released under the MIT license (see `LICENSE`) - But it was developed without a reference and no
copyright audit has been performed on the LLM-generated portions.

## What It Records

Each trace row is tab-separated:

```text
run_id side thread_id seq function input_hash output_hash input_len output_len elapsed_ns file line
```

When `TRACEHASH_VALUES=1` is set, rows get one additional debug column with
primitive input/output values and byte-slice summaries. This is intended for
local diagnosis after a hash mismatch has been localized; the default remains
hash-only.

The important comparison key is:

```text
function + input_hash -> output_hash
```

If one side calls a function more often, control flow differs. If the same
function and input hash produce different output hashes, the function may not be
equivalent for that input.

## Design Rules

Only hash canonical data that means the same thing in both languages:

- Use little-endian integer encodings.
- Hash floating-point values by raw IEEE-754 bits when checking bitwise parity.
- Use quantized float helpers when you need to distinguish tiny numeric drift
  from meaningful algorithmic differences. Rust and C quantization use the same
  `float` divide, add/subtract `0.5f`, then truncate rule so quantized hashes are
  comparable across both sides.
- Hash slices with an explicit length before bytes.
- Do not hash pointer addresses, allocation capacities, struct padding, or map
  iteration order.
- For structs, hash fields explicitly in a stable order.
- For impure functions, include every relevant external input in the input hash,
  including sequence bytes, model identifiers, RNG seed/state, thresholds, and
  mode flags.

## Rust Usage

Add the crate as an optional dependency while instrumenting a project:

```toml
[dependencies]
tracehash-rs = { version = "0.1", optional = true }

[features]
tracehash = ["dep:tracehash-rs"]
```

Instrument a function behind the feature:

```rust
#[cfg(feature = "tracehash")]
{
    let mut th = tracehash::th_call!("score_domain_envelope");
    th.input_usize(seq_len);
    th.input_usize(model_len);
    th.input_bytes(&sequence[1..=seq_len]);
    th.output_f32(env_score);
    th.output_f32_quant(env_score, 1.0e-5);
    th.output_u64(domain_count as u64);
    th.finish();
}
```

Build and run with tracing enabled:

```sh
cargo build --release --features tracehash

TRACEHASH_OUT=/tmp/rust.tsv TRACEHASH_SIDE=rust TRACEHASH_RUN_ID=case1 \
  target/release/my-rust-program args...
```

If `TRACEHASH_OUT` is not set, tracing is effectively disabled.
Set `TRACEHASH_VALUES=1` when you need readable scalar values for a narrow
probe or when a project-specific parity workflow says to keep both sides in
that mode. Byte slices are still summarized as `len:hash`, not emitted
verbatim.

### Deriving Stable Rust Hashes

For wider instrumentation, prefer grouping related inputs or outputs into small
probe structs and deriving `TraceHash`:

```rust
#[derive(tracehash::TraceHash)]
struct PipelineDecision {
    seq_len: usize,
    model_len: usize,
    score: f32,
    baseline: f32,
    pvalue: f32,
    passed: bool,
}

#[cfg(feature = "tracehash")]
{
    let decision = PipelineDecision {
        seq_len,
        model_len,
        score,
        baseline,
        pvalue: pvalue as f32,
        passed,
    };
    let mut th = tracehash::th_call!("pipeline_bias_decision");
    th.input_bytes(&sequence[1..=seq_len]);
    th.output_value(&decision);
    th.finish();
}
```

`#[derive(TraceHash)]` hashes named struct fields in declaration order, including
field names. This is useful for Rust-side breadth and consistency. For
Rust-vs-C parity, the C probe must emit fields in the same canonical order and
with the same primitive encodings.

For lighter ad hoc probes, use named scalar fields. These are useful when you
do not want to define a full struct but still want the hash to say what each
scalar means:

```rust
#[cfg(feature = "tracehash")]
{
    let mut th = tracehash::th_call!("pipeline_msv_decision");
    th.input_field("seq_len", &seq_len);
    th.input_field("model_len", &model_len);
    th.output_field("score", &score);
    th.output_field("passed", &passed);
    th.finish();
}
```

Rust `derive` macros apply to data types, not function bodies. A future
`#[tracehash::trace]` attribute macro could wrap simple functions automatically,
but manual probes are still better for hot kernels and for choosing exactly
which external inputs are part of a pure-function identity.

## C Usage

Include the C header only in instrumented builds:

```c
#ifdef TRACEHASH
#include "tracehash_c.h"
#endif
```

Instrument the matching C function with the same function name and the same
canonical field order:

```c
#ifdef TRACEHASH
{
  TH_CALL("score_domain_envelope");
  TH_IN_U64((uint64_t)seq_len);
  TH_IN_U64((uint64_t)model_len);
  TH_IN_BYTES(sequence + 1, (size_t)seq_len);
  TH_OUT_F32(env_score);
  TH_OUT_F32_Q(env_score, 1.0e-5f);
  TH_OUT_U64((uint64_t)domain_count);
  TH_FINISH();
}
#endif
```

`TH_CALL` declares a local variable named `th_call`. If you emit more than one
probe from the same C block, either wrap each probe in its own `{ ... }` scope
or use the explicit-handle macros:

```c
TH_CALL_N(msv_call, "pipeline_msv_decision");
TH_IN_U64_TO(&msv_call, seq_len);
TH_OUT_BOOL_TO(&msv_call, passed);
TH_FINISH_TO(&msv_call);
```

### Struct Helpers for C

To match a Rust `#[derive(tracehash::TraceHash)]` struct, define a field-list
macro and generate input/output helpers:

```c
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

{
  PipelineDecision decision = {seq_len, model_len, score, passed};
  TH_CALL("pipeline_msv_decision");
  TH_OUT_STRUCT(PipelineDecision, &decision);
  TH_FINISH();
}
```

`TH_DEFINE_STRUCT_HASH` emits two static functions for the type:
`tracehash_input_struct_Type()` and `tracehash_output_struct_Type()`. The type
name and field names are included in the hash, matching Rust derive behavior.
The helper currently expects a simple C identifier as the type name.

For lighter ad hoc probes, use the named scalar field macros. The field names
and primitive encodings match Rust `input_field()` / `output_field()`:

```c
TH_CALL("pipeline_msv_decision");
TH_IN_FIELD_U64("seq_len", seq_len);
TH_IN_FIELD_U64("model_len", model_len);
TH_OUT_FIELD_F32("score", score);
TH_OUT_FIELD_BOOL("passed", passed);
TH_FINISH();
```

## C++ Usage

For C++, include `tracehash_cpp.hpp` to use an RAII wrapper around the C API:

```cpp
#include "tracehash_cpp.hpp"

void score_candidate(const Sequence& seq, float score) {
  TRACEHASH_CALL("score_candidate");
  th_call.input_u64(seq.length());
  th_call.output_f32(score);
}
```

The destructor calls `finish()`, so early returns still emit a row. Use
`TRACEHASH_CALL_N(name, "function")` for multiple probes in one scope. The raw
C handle is available as `call.raw()` when you want to reuse the C struct
helpers from C++.

Compile and link:

```sh
cc -DTRACEHASH -Itracehash/c -c tracehash/c/tracehash_c.c -o /tmp/tracehash_c.o
cc -DTRACEHASH -Itracehash/c -o my-c-program my-c-program.o /tmp/tracehash_c.o -lpthread
```

Run:

```sh
TRACEHASH_OUT=/tmp/c.tsv TRACEHASH_SIDE=c TRACEHASH_RUN_ID=case1 \
  ./my-c-program args...
```

## Compare Traces

Install the comparator once:

```sh
cargo install tracehash-rs
```

Run the comparator:

```sh
tracehash-compare /tmp/rust.tsv /tmp/c.tsv
```

Useful filters:

```sh
tracehash-compare --only score_domain_forward,score_domain_null2 --first 50 \
  /tmp/rust.tsv /tmp/c.tsv

tracehash-compare --skip oprofile_xf_bits /tmp/rust.tsv /tmp/c.tsv

tracehash-compare --left-label rust --right-label c --summary-only \
  /tmp/rust.tsv /tmp/c.tsv
```

The comparator reports:

- Per-function call count differences.
- First occurrence-level differences by `function + input_hash + occurrence`.
- Inputs present on one side but not the other.
- Same-input output mismatches.
- Pair-difference totals grouped by function.

Typical interpretation:

```text
count differences:
  domain_envelope_candidate: left=588 right=586

pair differences by function:
  domain_decoding_summary: missing_inputs=0 output_mismatches=483
```

This means both sides reached `domain_decoding_summary` for the same inputs, but
the hashed outputs differ. Later call-count differences are probably downstream.
Use `--only` to focus on the earliest suspicious probe family and `--first N`
to print more occurrence-level mismatches, including the debug value column when
the trace was produced with `TRACEHASH_VALUES=1`. Use `--summary-only` for very
large traces when you only need counts and grouped totals.

## Agent Handoff Checklist

When giving `tracehash` to another debugging agent, point it at this checklist
first. Most bad comparisons come from under-specified input hashes.

1. Build both implementations from the same source state and use one
   `TRACEHASH_RUN_ID` per test case.
2. Give paired probes exactly the same function name on both sides.
3. Treat the input hash as a pure-function identity. Include every value that
   can affect the output, not just the arguments visible in the local function
   signature.
4. For sequence/model algorithms, include the relevant sequence bytes, model
   identity or model bytes/hash, window coordinates, mode flags, thresholds, and
   RNG state when applicable.
5. Do not compare probes whose inputs omit important context. For example,
   `seq_len + model_len + i + j` is not enough for domain scoring because many
   different sequences can share those values.
6. Prefer paired raw and quantized float outputs while debugging. Raw output
   proves bitwise parity; quantized output shows whether a mismatch is tiny
   numeric drift or a larger algorithmic difference. If the raw float bits match
   but a quantized helper differs, treat it as a tracehash bug.
7. Start with high-level summary probes, then add row/branch/state probes only
   around the first mismatching function.
8. Rebuild C without `TRACEHASH` before timing or normal correctness runs.

For HMMER specifically, the current useful probe families are:

- `pipeline_*_decision`: filter-level branch decisions and score thresholds.
- `domain_*_summary`: domain-definition region/cluster/envelope summaries.
- `simd_forward_*` and `simd_backward_*`: full-sequence SIMD parser anchors.
- `score_domain_forward_*`: isolated-envelope Forward anchors used during
  domain rescoring.
- `score_domain_null2` and `score_domain_oa`: downstream domain rescoring
  outputs after posterior decoding/null2/OA.

The current known HMMER workflow is to compare Pkinase against
`human_swissprot_2k.fasta`, then inspect `pair differences by function`. If
early rows match and later rows diverge, add a tighter row ladder or per-state
probes around the first bad row. If call counts differ, inspect branch/decision
probes before trusting downstream score mismatches.

## HMMER Example

This repository currently wires `tracehash` into Rust and C HMMER pipeline and
domain-definition code.

Build the Rust port:

```sh
cargo build --release --features tracehash
```

Build an instrumented C `hmmsearch`:

```sh
tracehash/scripts/build-c-hmmsearch.sh
```

The helper rebuilds each C object that currently contains `TRACEHASH` probes,
including the hot SIMD Forward/Backward object
`hmmer/src/impl_sse/fwdback.o` and SIMD posterior-decoding object
`hmmer/src/impl_sse/decoding.o`. It also rebuilds the SIMD optimized-profile
object, `hmmer/src/impl_sse/p7_oprofile.o`, for profile table parity probes.
Generic profile configuration probes rebuild `hmmer/src/modelconfig.o`.
When adding probes to another C object, update the helper in the same change so
comparisons do not silently miss that trace surface.

Run the same search on both sides:

```sh
TRACEHASH_OUT=target/tracehash-runs/ref.rust.tsv TRACEHASH_SIDE=rust TRACEHASH_VALUES=1 \
  target/release/hmmer search --noali \
  --tblout target/tracehash-runs/ref.rust.tbl --domtblout target/tracehash-runs/ref.rust.domtbl \
  test_data/Pkinase_pfam.hmm test_data/human_swissprot_2k.fasta \
  >target/tracehash-runs/ref.rust.out

TRACEHASH_OUT=target/tracehash-runs/ref.c.tsv TRACEHASH_SIDE=c TRACEHASH_VALUES=1 \
  hmmer/src/hmmsearch --noali \
  --tblout target/tracehash-runs/ref.c.tbl --domtblout target/tracehash-runs/ref.c.domtbl \
  test_data/Pkinase_pfam.hmm test_data/human_swissprot_2k.fasta \
  >target/tracehash-runs/ref.c.out
```

Use the same `TRACEHASH_VALUES` setting on both sides for bitwise HMMER
diagnostics. The extra value column is not part of the comparison key, but
keeping the runtime instrumentation mode identical avoids chasing
instrumentation-mode artifacts in extremely sensitive float paths.

Compare:

```sh
tracehash-compare \
  target/tracehash-runs/ref.rust.tsv target/tracehash-runs/ref.c.tsv
```

The full reference workflow is also available as one script:

```sh
tracehash/scripts/run-hmmer-reference.sh
```

It builds Rust with `--features tracehash`, builds C `hmmsearch` with
`TRACEHASH`, runs the Pkinase reference search on both sides with
`TRACEHASH_VALUES=1`, prints trace summaries and parsed `tblout` parity, then
rebuilds C without `TRACEHASH` before exiting. By default, large trace files
are written under `target/tracehash-runs` inside the repository. Override paths
with environment variables when needed:

```sh
TRACEHASH_WORKDIR=target/tracehash-runs PREFIX=target/tracehash-runs/my_case \
  HMM=path/to/model.hmm SEQS=path/to/seqs.fa \
  tracehash/scripts/run-hmmer-reference.sh
```

After an instrumented C run, rebuild C normally if you want to remove linked
trace symbols:

```sh
make -B -C hmmer/src/impl_sse fwdback.o decoding.o p7_oprofile.o CPPFLAGS=
make -B -C hmmer/src modelconfig.o p7_domaindef.o p7_pipeline.o CPPFLAGS=
make -C hmmer/src libhmmer.a hmmsearch
```

## Instrumentation Strategy

Start coarse, then move inward:

1. Add summary probes at high-level functions.
2. Compare call counts.
3. Add candidate/decision probes around branches that change counts.
4. Add hashed array summaries for numeric kernels.
5. Include enough identity in input hashes to avoid collapsing unrelated calls.

Good examples of identity fields:

- Sequence bytes or sequence accession.
- Model name, accession, or model length plus stable model hash.
- Window coordinates.
- RNG seed/state.
- Algorithm mode flags.

## Deep Mode: Full Structured Capture

Hash mode is cheap but opaque — a mismatch tells you *which* function diverged
for *which* canonical input, but not what the inputs/outputs actually were.
Deep mode captures the full structured `Value` for every input and output
into a per-function `.dclog` file, and ships a Rust-side replay harness that
lets you feed the recorded inputs into a ported function and structurally
diff the outputs.

Deep mode uses the same instrumentation points as hash mode. Positional
calls (`th.input_u64(x)`) still work — in deep mode they get auto-named
`in0`, `in1`, …. The new `_as` helpers attach an explicit field name
without changing the FNV hash stream, so existing `.tsv` comparisons stay
byte-compatible:

```rust
let mut th = tracehash::th_call!("pipeline_decision");
th.input_u64_as("seq_len", seq_len);
th.input_f32_as("score", score);
th.output_bool_as("passed", passed);
th.finish();
```

```c
TH_CALL("pipeline_decision");
TH_IN_U64_AS("seq_len",  seq_len);
TH_IN_F32_AS("score",    score);
TH_OUT_BOOL_AS("passed", passed);
TH_FINISH();
```

Enable deep mode on the Rust side with the `deep` cargo feature, then point
`TRACEHASH_DEEP_DIR` at a directory. Both sides can emit hash TSV and dclog
files simultaneously — the TSV gains a new `deep_seq` column that points at
the matching dclog entry, so you can jump from a hash-level mismatch straight
to the structured values:

```toml
[dependencies]
tracehash-rs = { version = "0.1", features = ["deep"], optional = true }
```

```sh
TRACEHASH_OUT=runs/rust.tsv TRACEHASH_SIDE=rust \
TRACEHASH_DEEP_DIR=runs/rust.deep TRACEHASH_DEEP_MODE=all \
  target/release/my-rust-port args...

TRACEHASH_OUT=runs/c.tsv TRACEHASH_SIDE=c \
TRACEHASH_DEEP_DIR=runs/c.deep TRACEHASH_DEEP_MODE=all \
  ./my-c-program args...
```

Replay the C-recorded inputs against the Rust port:

```rust
tracehash::deep::replay_assert(
    "runs/c.deep/pipeline_decision.dclog",
    |view| {
        let seq_len: u64 = view.input("seq_len")?.as_u64()?;
        let score:   f32 = view.input("score")?.as_f32()?;
        let expected: bool = view.output("passed")?.as_bool()?;

        let actual = my_rust_port::pipeline_decision(seq_len, score);
        let mut diff = tracehash::deep::Diff::new();
        if actual != expected {
            diff.push("passed", format!("expected {expected}, got {actual}"));
        }
        Ok(diff)
    },
);
```

Deep-mode env vars:

- `TRACEHASH_DEEP_DIR` — activates deep capture; one `.dclog` per function.
- `TRACEHASH_DEEP_MODE` — `first:N` (default `first:100`), `firstlast:N`,
  `prob:P[:first=N][:last=0|1]`, or `all`. C side currently supports
  `first:N` and `all`.
- `TRACEHASH_DEEP_SEED` — seed for the probabilistic sampler (Rust only).
- `TRACEHASH_COMPRESS` — zstd level 0..22 (Rust only, default 0). The Rust
  reader auto-detects raw vs. compressed dclog.
- `TRACEHASH_DEEP_ONLY` — Rust-only allowlist of function names.

The wire format is byte-compatible with the deep-comparator crate's `.dclog`
files. Deep-comparator is being deprecated in favor of this functionality.

## Current Limitations

- Hash-mode storage is TSV, not SQLite. Deep-mode storage is the binary
  dclog format (optionally zstd-compressed).
- Hashing uses FNV-1a 64-bit for simplicity; this is not cryptographic.
- Float hashes are raw-bit hashes only. Tiny numeric drift appears as a mismatch.
  Quantized float helpers are available for tolerance-oriented probes, but they
  should not replace raw float probes when bitwise parity is the goal.
- The C macro currently uses a fixed local variable name; repeated probes in one
  C block need explicit scopes.
- Thread order is not globally stable. The comparator does not compare global
  row order; its occurrence-level report is intended for deterministic single
  thread runs or for probes whose same-input occurrence order is meaningful.
- Deep mode currently captures value-copy snapshots only — pointer-identity
  aliasing (`Shared`/`Ref`/`Weak`), exception outcomes, and schema-aware struct
  entries from deep-comparator are not yet wired through the probe API, though
  the wire format already supports them.
- C-side deep mode supports `first:N` and `all` sampling only; `firstlast` and
  `prob` policies fall back to the default.

## Release Roadmap

Before publishing as an independent crate:

- Add `TRACEHASH_LEVEL` or `TRACEHASH_FILTER` runtime filtering.
- Add an auto-unique C call macro or handle-style API for repeated probes in one
  C block.
- Add an optional Rust `#[tracehash::trace]` attribute macro for simple function
  entry/exit probes.
- Add optional SQLite output.
- Add a schema/version header.
- Add stable array helpers for `f32`, `u8`, `u32`, and packed structs.
- Add CMake/pkg-config examples for the C shim.
- Add tests proving Rust and C hash streams match for every primitive helper.
