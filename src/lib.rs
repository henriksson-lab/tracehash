//! Cross-language function call tracing by canonical input/output hashes,
//! with optional full-fidelity deep-log capture.
//!
//! Set `TRACEHASH_OUT=/path/to/run.tsv` to enable the hash-only TSV stream.
//! Optional `TRACEHASH_SIDE` and `TRACEHASH_RUN_ID` values are copied into
//! each row.
//!
//! When the `deep` feature is enabled and `TRACEHASH_DEEP_DIR=<dir>` is set,
//! every probe additionally emits a structured entry into a per-function
//! `.dclog` file. The TSV row's `deep_seq` column points at the matching
//! dclog entry (or `-` when not recorded).

use std::fmt::Write as _;
use std::fs::File;
use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

mod stable_hash;

#[cfg(feature = "deep")]
pub mod deep;
#[cfg(feature = "deep")]
pub mod spec;

pub use stable_hash::Fnv64;
#[cfg(feature = "derive")]
pub use tracehash_rs_derive::TraceHash;

#[cfg(feature = "deep")]
pub use spec::{Outcome, Value};

static WRITER: OnceLock<Mutex<Option<TraceWriter>>> = OnceLock::new();
static SEQ: AtomicU64 = AtomicU64::new(0);

struct TraceWriter {
    file: File,
    side: String,
    run_id: String,
    values: bool,
}

fn writer() -> &'static Mutex<Option<TraceWriter>> {
    WRITER.get_or_init(|| {
        let out = match std::env::var("TRACEHASH_OUT") {
            Ok(path) if !path.is_empty() => path,
            _ => return Mutex::new(None),
        };
        let file = File::create(out).ok();
        let side = std::env::var("TRACEHASH_SIDE").unwrap_or_else(|_| "rust".to_string());
        let run_id = std::env::var("TRACEHASH_RUN_ID").unwrap_or_else(|_| "default".to_string());
        let values = std::env::var("TRACEHASH_VALUES")
            .map(|value| !value.is_empty() && value != "0")
            .unwrap_or(false);
        Mutex::new(file.map(|file| TraceWriter {
            file,
            side,
            run_id,
            values,
        }))
    })
}

#[must_use]
pub fn enabled() -> bool {
    let tsv = writer()
        .lock()
        .map(|guard| guard.is_some())
        .unwrap_or(false);
    #[cfg(feature = "deep")]
    {
        tsv || deep::enabled()
    }
    #[cfg(not(feature = "deep"))]
    {
        tsv
    }
}

fn tsv_enabled() -> bool {
    writer()
        .lock()
        .map(|guard| guard.is_some())
        .unwrap_or(false)
}

fn values_enabled() -> bool {
    writer()
        .lock()
        .map(|guard| guard.as_ref().map(|writer| writer.values).unwrap_or(false))
        .unwrap_or(false)
}

pub struct Call {
    function: &'static str,
    file: &'static str,
    line: u32,
    input: Fnv64,
    output: Fnv64,
    input_len: u64,
    output_len: u64,
    start: Instant,
    active: bool,
    values: Option<String>,

    #[cfg(feature = "deep")]
    deep_active: bool,
    #[cfg(feature = "deep")]
    deep_inputs: Vec<(String, Value)>,
    #[cfg(feature = "deep")]
    deep_outputs: Vec<(String, Value)>,
    #[cfg(feature = "deep")]
    input_counter: u32,
    #[cfg(feature = "deep")]
    output_counter: u32,
}

impl Call {
    #[inline]
    pub fn new(function: &'static str, file: &'static str, line: u32) -> Self {
        let tsv = tsv_enabled();
        #[cfg(feature = "deep")]
        let deep_active = deep::enabled();
        #[cfg(not(feature = "deep"))]
        let deep_active = false;
        let active = tsv || deep_active;

        let mut input = Fnv64::new();
        let mut output = Fnv64::new();
        if active {
            input.str(function);
            output.str(function);
        }
        Self {
            function,
            file,
            line,
            input,
            output,
            input_len: 0,
            output_len: 0,
            start: Instant::now(),
            active,
            values: if tsv && values_enabled() {
                Some(String::new())
            } else {
                None
            },
            #[cfg(feature = "deep")]
            deep_active,
            #[cfg(feature = "deep")]
            deep_inputs: Vec::new(),
            #[cfg(feature = "deep")]
            deep_outputs: Vec::new(),
            #[cfg(feature = "deep")]
            input_counter: 0,
            #[cfg(feature = "deep")]
            output_counter: 0,
        }
    }

    #[inline]
    fn push_value(&mut self, label: &str, value: impl std::fmt::Display) {
        if let Some(values) = self.values.as_mut() {
            if !values.is_empty() {
                values.push(';');
            }
            let _ = write!(values, "{label}={value}");
        }
    }

    #[cfg(feature = "deep")]
    #[inline]
    fn auto_in_name(&mut self) -> String {
        let name = format!("in{}", self.input_counter);
        self.input_counter += 1;
        name
    }

    #[cfg(feature = "deep")]
    #[inline]
    fn auto_out_name(&mut self) -> String {
        let name = format!("out{}", self.output_counter);
        self.output_counter += 1;
        name
    }

    #[cfg(feature = "deep")]
    #[inline]
    fn deep_push_input(&mut self, name: String, value: Value) {
        if self.deep_active {
            self.deep_inputs.push((name, value));
        }
    }

    #[cfg(feature = "deep")]
    #[inline]
    fn deep_push_output(&mut self, name: String, value: Value) {
        if self.deep_active {
            self.deep_outputs.push((name, value));
        }
    }

    // -- inputs ---------------------------------------------------------------

    #[inline]
    pub fn input_u64(&mut self, value: u64) {
        if self.active {
            self.input.u8(b'U');
            self.input.u64(value);
            self.input_len += 1;
            self.push_value("IU64", value);
            #[cfg(feature = "deep")]
            {
                let name = self.auto_in_name();
                self.deep_push_input(name, Value::U64(value));
            }
        }
    }

    #[inline]
    pub fn input_i64(&mut self, value: i64) {
        if self.active {
            self.input.u8(b'U');
            self.input.u64(value as u64);
            self.input_len += 1;
            self.push_value("IU64", value as u64);
            #[cfg(feature = "deep")]
            {
                let name = self.auto_in_name();
                self.deep_push_input(name, Value::I64(value));
            }
        }
    }

    #[inline]
    pub fn input_usize(&mut self, value: usize) {
        self.input_u64(value as u64);
    }

    #[inline]
    pub fn input_bool(&mut self, value: bool) {
        if self.active {
            self.input.u8(b'B');
            self.input.u8(value as u8);
            self.input_len += 1;
            self.push_value("IBOOL", value as u8);
            #[cfg(feature = "deep")]
            {
                let name = self.auto_in_name();
                self.deep_push_input(name, Value::Bool(value));
            }
        }
    }

    #[inline]
    pub fn input_f32(&mut self, value: f32) {
        if self.active {
            self.input.u8(b'F');
            self.input.u32(value.to_bits());
            self.input_len += 1;
            self.push_value(
                "IF32",
                format_args!("{:08x}/{:.9e}", value.to_bits(), value),
            );
            #[cfg(feature = "deep")]
            {
                let name = self.auto_in_name();
                self.deep_push_input(name, Value::F32(value));
            }
        }
    }

    #[inline]
    pub fn input_f64(&mut self, value: f64) {
        if self.active {
            self.input.u8(b'D');
            self.input.u64(value.to_bits());
            self.input_len += 1;
            self.push_value(
                "IF64",
                format_args!("{:016x}/{:.17e}", value.to_bits(), value),
            );
            #[cfg(feature = "deep")]
            {
                let name = self.auto_in_name();
                self.deep_push_input(name, Value::F64(value));
            }
        }
    }

    #[inline]
    pub fn input_f32_quant(&mut self, value: f32, quantum: f32) {
        if self.active {
            self.input.u8(b'Q');
            self.input.u32(quantum.to_bits());
            self.input.u64(quantize_f32(value, quantum) as u64);
            self.input_len += 1;
            self.push_value(
                "IF32Q",
                format_args!("{:08x}/{}", quantum.to_bits(), quantize_f32(value, quantum)),
            );
            #[cfg(feature = "deep")]
            {
                let name = self.auto_in_name();
                self.deep_push_input(name, Value::F32(value));
            }
        }
    }

    #[inline]
    pub fn input_bytes(&mut self, bytes: &[u8]) {
        if self.active {
            self.input.u8(b'Y');
            self.input.u64(bytes.len() as u64);
            self.input.bytes(bytes);
            self.input_len += bytes.len() as u64;
            let mut hash = Fnv64::new();
            hash.bytes(bytes);
            self.push_value(
                "IBYTES",
                format_args!("{}:{:016x}", bytes.len(), hash.finish()),
            );
            #[cfg(feature = "deep")]
            {
                let name = self.auto_in_name();
                self.deep_push_input(name, Value::Bytes(bytes.to_vec()));
            }
        }
    }

    #[inline]
    pub fn input_value<T: TraceHash + ?Sized>(&mut self, value: &T) {
        if self.active {
            self.input.u8(b'V');
            value.trace_hash(&mut self.input);
            self.input_len += 1;
        }
    }

    #[inline]
    pub fn input_field<T: TraceHash + ?Sized>(&mut self, field: &'static str, value: &T) {
        if self.active {
            self.input.u8(b'G');
            self.input.str(field);
            value.trace_hash(&mut self.input);
            self.input_len += 1;
            self.push_value("IFIELD", field);
        }
    }

    // -- `_as` named inputs: hash-identical to positional, name flows only
    // to the deep log. Use these when you want the dclog entry to carry
    // a meaningful field name without changing the hash stream.

    #[cfg(feature = "deep")]
    #[inline]
    pub fn input_u64_as(&mut self, name: &str, value: u64) {
        if self.active {
            self.input.u8(b'U');
            self.input.u64(value);
            self.input_len += 1;
            self.push_value("IU64", value);
            self.input_counter += 1;
            self.deep_push_input(name.to_string(), Value::U64(value));
        }
    }

    #[cfg(feature = "deep")]
    #[inline]
    pub fn input_i64_as(&mut self, name: &str, value: i64) {
        if self.active {
            self.input.u8(b'U');
            self.input.u64(value as u64);
            self.input_len += 1;
            self.push_value("IU64", value as u64);
            self.input_counter += 1;
            self.deep_push_input(name.to_string(), Value::I64(value));
        }
    }

    #[cfg(feature = "deep")]
    #[inline]
    pub fn input_usize_as(&mut self, name: &str, value: usize) {
        if self.active {
            self.input.u8(b'U');
            self.input.u64(value as u64);
            self.input_len += 1;
            self.push_value("IU64", value as u64);
            self.input_counter += 1;
            self.deep_push_input(name.to_string(), Value::U64(value as u64));
        }
    }

    #[cfg(feature = "deep")]
    #[inline]
    pub fn input_bool_as(&mut self, name: &str, value: bool) {
        if self.active {
            self.input.u8(b'B');
            self.input.u8(value as u8);
            self.input_len += 1;
            self.push_value("IBOOL", value as u8);
            self.input_counter += 1;
            self.deep_push_input(name.to_string(), Value::Bool(value));
        }
    }

    #[cfg(feature = "deep")]
    #[inline]
    pub fn input_f32_as(&mut self, name: &str, value: f32) {
        if self.active {
            self.input.u8(b'F');
            self.input.u32(value.to_bits());
            self.input_len += 1;
            self.push_value(
                "IF32",
                format_args!("{:08x}/{:.9e}", value.to_bits(), value),
            );
            self.input_counter += 1;
            self.deep_push_input(name.to_string(), Value::F32(value));
        }
    }

    #[cfg(feature = "deep")]
    #[inline]
    pub fn input_f64_as(&mut self, name: &str, value: f64) {
        if self.active {
            self.input.u8(b'D');
            self.input.u64(value.to_bits());
            self.input_len += 1;
            self.push_value(
                "IF64",
                format_args!("{:016x}/{:.17e}", value.to_bits(), value),
            );
            self.input_counter += 1;
            self.deep_push_input(name.to_string(), Value::F64(value));
        }
    }

    #[cfg(feature = "deep")]
    #[inline]
    pub fn input_bytes_as(&mut self, name: &str, bytes: &[u8]) {
        if self.active {
            self.input.u8(b'Y');
            self.input.u64(bytes.len() as u64);
            self.input.bytes(bytes);
            self.input_len += bytes.len() as u64;
            let mut hash = Fnv64::new();
            hash.bytes(bytes);
            self.push_value(
                "IBYTES",
                format_args!("{}:{:016x}", bytes.len(), hash.finish()),
            );
            self.input_counter += 1;
            self.deep_push_input(name.to_string(), Value::Bytes(bytes.to_vec()));
        }
    }

    // -- outputs --------------------------------------------------------------

    #[inline]
    pub fn output_u64(&mut self, value: u64) {
        if self.active {
            self.output.u8(b'U');
            self.output.u64(value);
            self.output_len += 1;
            self.push_value("OU64", value);
            #[cfg(feature = "deep")]
            {
                let name = self.auto_out_name();
                self.deep_push_output(name, Value::U64(value));
            }
        }
    }

    #[inline]
    pub fn output_i64(&mut self, value: i64) {
        if self.active {
            self.output.u8(b'U');
            self.output.u64(value as u64);
            self.output_len += 1;
            self.push_value("OU64", value as u64);
            #[cfg(feature = "deep")]
            {
                let name = self.auto_out_name();
                self.deep_push_output(name, Value::I64(value));
            }
        }
    }

    #[inline]
    pub fn output_bool(&mut self, value: bool) {
        if self.active {
            self.output.u8(b'B');
            self.output.u8(value as u8);
            self.output_len += 1;
            self.push_value("OBOOL", value as u8);
            #[cfg(feature = "deep")]
            {
                let name = self.auto_out_name();
                self.deep_push_output(name, Value::Bool(value));
            }
        }
    }

    #[inline]
    pub fn output_f32(&mut self, value: f32) {
        if self.active {
            self.output.u8(b'F');
            self.output.u32(value.to_bits());
            self.output_len += 1;
            self.push_value(
                "OF32",
                format_args!("{:08x}/{:.9e}", value.to_bits(), value),
            );
            #[cfg(feature = "deep")]
            {
                let name = self.auto_out_name();
                self.deep_push_output(name, Value::F32(value));
            }
        }
    }

    #[inline]
    pub fn output_f64(&mut self, value: f64) {
        if self.active {
            self.output.u8(b'D');
            self.output.u64(value.to_bits());
            self.output_len += 1;
            self.push_value(
                "OF64",
                format_args!("{:016x}/{:.17e}", value.to_bits(), value),
            );
            #[cfg(feature = "deep")]
            {
                let name = self.auto_out_name();
                self.deep_push_output(name, Value::F64(value));
            }
        }
    }

    #[inline]
    pub fn output_f32_quant(&mut self, value: f32, quantum: f32) {
        if self.active {
            self.output.u8(b'Q');
            self.output.u32(quantum.to_bits());
            self.output.u64(quantize_f32(value, quantum) as u64);
            self.output_len += 1;
            self.push_value(
                "OF32Q",
                format_args!("{:08x}/{}", quantum.to_bits(), quantize_f32(value, quantum)),
            );
            #[cfg(feature = "deep")]
            {
                let name = self.auto_out_name();
                self.deep_push_output(name, Value::F32(value));
            }
        }
    }

    #[inline]
    pub fn output_bytes(&mut self, bytes: &[u8]) {
        if self.active {
            self.output.u8(b'Y');
            self.output.u64(bytes.len() as u64);
            self.output.bytes(bytes);
            self.output_len += bytes.len() as u64;
            let mut hash = Fnv64::new();
            hash.bytes(bytes);
            self.push_value(
                "OBYTES",
                format_args!("{}:{:016x}", bytes.len(), hash.finish()),
            );
            #[cfg(feature = "deep")]
            {
                let name = self.auto_out_name();
                self.deep_push_output(name, Value::Bytes(bytes.to_vec()));
            }
        }
    }

    #[inline]
    pub fn output_value<T: TraceHash + ?Sized>(&mut self, value: &T) {
        if self.active {
            self.output.u8(b'V');
            value.trace_hash(&mut self.output);
            self.output_len += 1;
        }
    }

    #[inline]
    pub fn output_field<T: TraceHash + ?Sized>(&mut self, field: &'static str, value: &T) {
        if self.active {
            self.output.u8(b'G');
            self.output.str(field);
            value.trace_hash(&mut self.output);
            self.output_len += 1;
            self.push_value("OFIELD", field);
        }
    }

    // -- `_as` named outputs --------------------------------------------------

    #[cfg(feature = "deep")]
    #[inline]
    pub fn output_u64_as(&mut self, name: &str, value: u64) {
        if self.active {
            self.output.u8(b'U');
            self.output.u64(value);
            self.output_len += 1;
            self.push_value("OU64", value);
            self.output_counter += 1;
            self.deep_push_output(name.to_string(), Value::U64(value));
        }
    }

    #[cfg(feature = "deep")]
    #[inline]
    pub fn output_i64_as(&mut self, name: &str, value: i64) {
        if self.active {
            self.output.u8(b'U');
            self.output.u64(value as u64);
            self.output_len += 1;
            self.push_value("OU64", value as u64);
            self.output_counter += 1;
            self.deep_push_output(name.to_string(), Value::I64(value));
        }
    }

    #[cfg(feature = "deep")]
    #[inline]
    pub fn output_bool_as(&mut self, name: &str, value: bool) {
        if self.active {
            self.output.u8(b'B');
            self.output.u8(value as u8);
            self.output_len += 1;
            self.push_value("OBOOL", value as u8);
            self.output_counter += 1;
            self.deep_push_output(name.to_string(), Value::Bool(value));
        }
    }

    #[cfg(feature = "deep")]
    #[inline]
    pub fn output_f32_as(&mut self, name: &str, value: f32) {
        if self.active {
            self.output.u8(b'F');
            self.output.u32(value.to_bits());
            self.output_len += 1;
            self.push_value(
                "OF32",
                format_args!("{:08x}/{:.9e}", value.to_bits(), value),
            );
            self.output_counter += 1;
            self.deep_push_output(name.to_string(), Value::F32(value));
        }
    }

    #[cfg(feature = "deep")]
    #[inline]
    pub fn output_f64_as(&mut self, name: &str, value: f64) {
        if self.active {
            self.output.u8(b'D');
            self.output.u64(value.to_bits());
            self.output_len += 1;
            self.push_value(
                "OF64",
                format_args!("{:016x}/{:.17e}", value.to_bits(), value),
            );
            self.output_counter += 1;
            self.deep_push_output(name.to_string(), Value::F64(value));
        }
    }

    #[cfg(feature = "deep")]
    #[inline]
    pub fn output_bytes_as(&mut self, name: &str, bytes: &[u8]) {
        if self.active {
            self.output.u8(b'Y');
            self.output.u64(bytes.len() as u64);
            self.output.bytes(bytes);
            self.output_len += bytes.len() as u64;
            let mut hash = Fnv64::new();
            hash.bytes(bytes);
            self.push_value(
                "OBYTES",
                format_args!("{}:{:016x}", bytes.len(), hash.finish()),
            );
            self.output_counter += 1;
            self.deep_push_output(name.to_string(), Value::Bytes(bytes.to_vec()));
        }
    }

    /// Current running FNV hash over the input stream. Does not consume
    /// the call. Useful in tests.
    #[inline]
    pub fn current_input_hash(&self) -> u64 {
        self.input.finish()
    }

    /// Current running FNV hash over the output stream.
    #[inline]
    pub fn current_output_hash(&self) -> u64 {
        self.output.finish()
    }

    #[inline]
    pub fn finish(self) {
        if !self.active {
            return;
        }
        let elapsed_ns = self.start.elapsed().as_nanos();
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let thread_id = thread_hash();

        #[cfg(feature = "deep")]
        let deep_seq = if self.deep_active {
            deep::record(self.function, self.deep_inputs, self.deep_outputs)
        } else {
            None
        };
        #[cfg(not(feature = "deep"))]
        let deep_seq: Option<u32> = None;

        let deep_seq_field = match deep_seq {
            Some(n) => n.to_string(),
            None => "-".to_string(),
        };

        let mut row = format!(
            "{}\t{}\t{}\t{}\t{}\t{:016x}\t{:016x}\t{}\t{}\t{}\t{}\t{}\t{}",
            "{run_id}",
            "{side}",
            thread_id,
            seq,
            self.function,
            self.input.finish(),
            self.output.finish(),
            self.input_len,
            self.output_len,
            elapsed_ns,
            self.file,
            self.line,
            deep_seq_field,
        );
        if let Some(values) = self.values {
            row.push('\t');
            row.push_str(&values);
        }
        row.push('\n');
        if let Ok(mut guard) = writer().lock() {
            if let Some(writer) = guard.as_mut() {
                let row = row
                    .replace("{run_id}", &writer.run_id)
                    .replace("{side}", &writer.side);
                let _ = writer.file.write_all(row.as_bytes());
            }
        }
    }
}

pub trait TraceHash {
    fn trace_hash(&self, state: &mut Fnv64);
}

macro_rules! impl_tracehash_unsigned {
    ($($ty:ty),* $(,)?) => {
        $(
            impl TraceHash for $ty {
                #[inline]
                fn trace_hash(&self, state: &mut Fnv64) {
                    state.u8(b'U');
                    state.u64(*self as u64);
                }
            }
        )*
    };
}

macro_rules! impl_tracehash_signed {
    ($($ty:ty),* $(,)?) => {
        $(
            impl TraceHash for $ty {
                #[inline]
                fn trace_hash(&self, state: &mut Fnv64) {
                    state.u8(b'U');
                    state.u64(*self as i64 as u64);
                }
            }
        )*
    };
}

impl_tracehash_unsigned!(u8, u16, u32, u64, usize);
impl_tracehash_signed!(i8, i16, i32, i64, isize);

impl TraceHash for bool {
    #[inline]
    fn trace_hash(&self, state: &mut Fnv64) {
        state.u8(b'B');
        state.u8(*self as u8);
    }
}

impl TraceHash for f32 {
    #[inline]
    fn trace_hash(&self, state: &mut Fnv64) {
        state.u8(b'F');
        state.u32(self.to_bits());
    }
}

impl TraceHash for f64 {
    #[inline]
    fn trace_hash(&self, state: &mut Fnv64) {
        state.u8(b'D');
        state.u64(self.to_bits());
    }
}

impl TraceHash for str {
    #[inline]
    fn trace_hash(&self, state: &mut Fnv64) {
        state.u8(b'S');
        state.str(self);
    }
}

impl TraceHash for String {
    #[inline]
    fn trace_hash(&self, state: &mut Fnv64) {
        self.as_str().trace_hash(state);
    }
}

impl<T: TraceHash> TraceHash for [T] {
    #[inline]
    fn trace_hash(&self, state: &mut Fnv64) {
        state.u8(b'L');
        state.u64(self.len() as u64);
        for value in self {
            value.trace_hash(state);
        }
    }
}

impl<T: TraceHash> TraceHash for Vec<T> {
    #[inline]
    fn trace_hash(&self, state: &mut Fnv64) {
        self.as_slice().trace_hash(state);
    }
}

impl<T: TraceHash + ?Sized> TraceHash for &T {
    #[inline]
    fn trace_hash(&self, state: &mut Fnv64) {
        (*self).trace_hash(state);
    }
}

impl<T: TraceHash> TraceHash for Option<T> {
    #[inline]
    fn trace_hash(&self, state: &mut Fnv64) {
        match self {
            Some(value) => {
                state.u8(b'1');
                value.trace_hash(state);
            }
            None => state.u8(b'0'),
        }
    }
}

fn thread_hash() -> u64 {
    let text = format!("{:?}", std::thread::current().id());
    let mut hash = Fnv64::new();
    hash.str(&text);
    hash.finish()
}

fn quantize_f32(value: f32, quantum: f32) -> i64 {
    if value.is_nan() {
        i64::MIN
    } else if value == f32::INFINITY {
        i64::MAX
    } else if value == f32::NEG_INFINITY {
        i64::MIN + 1
    } else if quantum > 0.0 {
        let scaled = value / quantum;
        if scaled >= 0.0 {
            (scaled + 0.5) as i64
        } else {
            (scaled - 0.5) as i64
        }
    } else {
        value.to_bits() as i64
    }
}

#[macro_export]
macro_rules! th_call {
    ($name:literal) => {
        $crate::Call::new($name, file!(), line!())
    };
}
