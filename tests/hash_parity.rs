//! `_as` named probes must hash-identically to their positional siblings —
//! the name is metadata for the deep log only, and must not leak into the
//! FNV hash stream.

#![cfg(feature = "deep")]

#[test]
fn named_probes_match_positional_hashes() {
    let (positional_input, positional_output) = {
        let mut th = tracehash::Call::new("hash_parity", "test.rs", 1);
        th.input_u64(42);
        th.input_f32(3.5);
        th.output_bool(true);
        (th.current_input_hash(), th.current_output_hash())
    };
    let (named_input, named_output) = {
        let mut th = tracehash::Call::new("hash_parity", "test.rs", 1);
        th.input_u64_as("seq_len", 42);
        th.input_f32_as("score", 3.5);
        th.output_bool_as("passed", true);
        (th.current_input_hash(), th.current_output_hash())
    };
    assert_eq!(positional_input, named_input);
    assert_eq!(positional_output, named_output);
}

#[test]
fn bytes_named_probe_matches_positional() {
    let payload = b"some bytes here";
    let p = {
        let mut th = tracehash::Call::new("bytes_parity", "test.rs", 1);
        th.input_bytes(payload);
        th.current_input_hash()
    };
    let n = {
        let mut th = tracehash::Call::new("bytes_parity", "test.rs", 1);
        th.input_bytes_as("payload", payload);
        th.current_input_hash()
    };
    assert_eq!(p, n);
}
