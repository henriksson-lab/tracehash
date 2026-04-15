use tracehash::TraceHash;

#[derive(TraceHash)]
struct ProbeInput {
    seq_len: usize,
    model_len: usize,
    score: f32,
    passed: bool,
}

#[test]
fn derive_hashes_fields_in_declaration_order() {
    let value = ProbeInput {
        seq_len: 116,
        model_len: 262,
        score: 12.5,
        passed: true,
    };

    let mut derived = tracehash::Fnv64::new();
    value.trace_hash(&mut derived);

    let mut manual = tracehash::Fnv64::new();
    manual.str("ProbeInput");
    manual.str("seq_len");
    value.seq_len.trace_hash(&mut manual);
    manual.str("model_len");
    value.model_len.trace_hash(&mut manual);
    manual.str("score");
    value.score.trace_hash(&mut manual);
    manual.str("passed");
    value.passed.trace_hash(&mut manual);

    assert_eq!(derived.finish(), manual.finish());
}
