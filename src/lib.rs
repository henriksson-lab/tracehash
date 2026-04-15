//! Cross-language function call tracing by canonical input/output hashes.
//!
//! Set `TRACEHASH_OUT=/path/to/run.tsv` to enable recording. Optional
//! `TRACEHASH_SIDE` and `TRACEHASH_RUN_ID` values are copied into each row.

use std::fs::File;
use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

mod stable_hash;

pub use stable_hash::Fnv64;
#[cfg(feature = "derive")]
pub use tracehash_derive::TraceHash;

static WRITER: OnceLock<Mutex<Option<TraceWriter>>> = OnceLock::new();
static SEQ: AtomicU64 = AtomicU64::new(0);

struct TraceWriter {
    file: File,
    side: String,
    run_id: String,
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
        Mutex::new(file.map(|file| TraceWriter { file, side, run_id }))
    })
}

#[must_use]
pub fn enabled() -> bool {
    writer()
        .lock()
        .map(|guard| guard.is_some())
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
}

impl Call {
    #[inline]
    pub fn new(function: &'static str, file: &'static str, line: u32) -> Self {
        let active = enabled();
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
        }
    }

    #[inline]
    pub fn input_u64(&mut self, value: u64) {
        if self.active {
            self.input.u8(b'U');
            self.input.u64(value);
            self.input_len += 1;
        }
    }

    #[inline]
    pub fn input_i64(&mut self, value: i64) {
        self.input_u64(value as u64);
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
        }
    }

    #[inline]
    pub fn input_f32(&mut self, value: f32) {
        if self.active {
            self.input.u8(b'F');
            self.input.u32(value.to_bits());
            self.input_len += 1;
        }
    }

    #[inline]
    pub fn input_f32_quant(&mut self, value: f32, quantum: f32) {
        if self.active {
            self.input.u8(b'Q');
            self.input.u32(quantum.to_bits());
            self.input.u64(quantize_f32(value, quantum) as u64);
            self.input_len += 1;
        }
    }

    #[inline]
    pub fn input_bytes(&mut self, bytes: &[u8]) {
        if self.active {
            self.input.u8(b'Y');
            self.input.u64(bytes.len() as u64);
            self.input.bytes(bytes);
            self.input_len += bytes.len() as u64;
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
    pub fn output_u64(&mut self, value: u64) {
        if self.active {
            self.output.u8(b'U');
            self.output.u64(value);
            self.output_len += 1;
        }
    }

    #[inline]
    pub fn output_i64(&mut self, value: i64) {
        self.output_u64(value as u64);
    }

    #[inline]
    pub fn output_f32(&mut self, value: f32) {
        if self.active {
            self.output.u8(b'F');
            self.output.u32(value.to_bits());
            self.output_len += 1;
        }
    }

    #[inline]
    pub fn output_f32_quant(&mut self, value: f32, quantum: f32) {
        if self.active {
            self.output.u8(b'Q');
            self.output.u32(quantum.to_bits());
            self.output.u64(quantize_f32(value, quantum) as u64);
            self.output_len += 1;
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
    pub fn finish(self) {
        if !self.active {
            return;
        }
        let elapsed_ns = self.start.elapsed().as_nanos();
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let thread_id = thread_hash();
        let row = format!(
            "{}\t{}\t{}\t{}\t{}\t{:016x}\t{:016x}\t{}\t{}\t{}\t{}\t{}\n",
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
            self.line
        );
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
        (value / quantum).round() as i64
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
