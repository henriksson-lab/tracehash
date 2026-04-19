//! Wire format encoding/decoding.
//!
//! All multi-byte integers are little-endian. All lengths are `u32`. Strings
//! are UTF-8. Matches deep-comparator's format byte-for-byte.

use super::error::{Error, Result};
use super::header::LogHeader;
use super::tags::*;
use super::value::{Outcome, Value};
use super::{MAGIC, SPEC_VERSION};
use std::io::{Read, Write};

#[inline]
fn write_u8<W: Write>(w: &mut W, v: u8) -> Result<()> {
    w.write_all(&[v])?;
    Ok(())
}

#[inline]
fn write_u32<W: Write>(w: &mut W, v: u32) -> Result<()> {
    w.write_all(&v.to_le_bytes())?;
    Ok(())
}

#[inline]
fn write_u64<W: Write>(w: &mut W, v: u64) -> Result<()> {
    w.write_all(&v.to_le_bytes())?;
    Ok(())
}

#[inline]
fn write_str<W: Write>(w: &mut W, s: &str) -> Result<()> {
    let bytes = s.as_bytes();
    write_u32(w, bytes.len() as u32)?;
    w.write_all(bytes)?;
    Ok(())
}

#[inline]
fn read_exact<R: Read>(r: &mut R, buf: &mut [u8]) -> Result<()> {
    r.read_exact(buf).map_err(|e| match e.kind() {
        std::io::ErrorKind::UnexpectedEof => Error::UnexpectedEof,
        _ => Error::Io(e),
    })
}

#[inline]
fn read_u8<R: Read>(r: &mut R) -> Result<u8> {
    let mut b = [0u8; 1];
    read_exact(r, &mut b)?;
    Ok(b[0])
}

#[inline]
fn read_u32<R: Read>(r: &mut R) -> Result<u32> {
    let mut b = [0u8; 4];
    read_exact(r, &mut b)?;
    Ok(u32::from_le_bytes(b))
}

#[inline]
fn read_u64<R: Read>(r: &mut R) -> Result<u64> {
    let mut b = [0u8; 8];
    read_exact(r, &mut b)?;
    Ok(u64::from_le_bytes(b))
}

#[inline]
fn read_string<R: Read>(r: &mut R) -> Result<String> {
    let len = read_u32(r)? as usize;
    let mut v = vec![0u8; len];
    read_exact(r, &mut v)?;
    String::from_utf8(v).map_err(|_| Error::InvalidUtf8)
}

pub fn write_value<W: Write>(w: &mut W, v: &Value) -> Result<()> {
    match v {
        Value::I8(x) => {
            write_u8(w, TAG_I8)?;
            w.write_all(&x.to_le_bytes())?;
        }
        Value::I16(x) => {
            write_u8(w, TAG_I16)?;
            w.write_all(&x.to_le_bytes())?;
        }
        Value::I32(x) => {
            write_u8(w, TAG_I32)?;
            w.write_all(&x.to_le_bytes())?;
        }
        Value::I64(x) => {
            write_u8(w, TAG_I64)?;
            w.write_all(&x.to_le_bytes())?;
        }
        Value::U8(x) => {
            write_u8(w, TAG_U8)?;
            w.write_all(&x.to_le_bytes())?;
        }
        Value::U16(x) => {
            write_u8(w, TAG_U16)?;
            w.write_all(&x.to_le_bytes())?;
        }
        Value::U32(x) => {
            write_u8(w, TAG_U32)?;
            w.write_all(&x.to_le_bytes())?;
        }
        Value::U64(x) => {
            write_u8(w, TAG_U64)?;
            w.write_all(&x.to_le_bytes())?;
        }
        Value::F32(x) => {
            write_u8(w, TAG_F32)?;
            w.write_all(&x.to_le_bytes())?;
        }
        Value::F64(x) => {
            write_u8(w, TAG_F64)?;
            w.write_all(&x.to_le_bytes())?;
        }
        Value::Bool(x) => {
            write_u8(w, TAG_BOOL)?;
            w.write_all(&[*x as u8])?;
        }
        Value::Null => {
            write_u8(w, TAG_NULL)?;
        }
        Value::Bytes(b) => {
            write_u8(w, TAG_BYTES)?;
            write_u32(w, b.len() as u32)?;
            w.write_all(b)?;
        }
        Value::String(s) => {
            write_u8(w, TAG_STRING)?;
            write_str(w, s)?;
        }
        Value::Array(items) => {
            write_u8(w, TAG_ARRAY)?;
            write_u32(w, items.len() as u32)?;
            for item in items {
                write_value(w, item)?;
            }
        }
        Value::Struct { schema_id, fields } => {
            write_u8(w, TAG_STRUCT)?;
            write_u32(w, *schema_id)?;
            write_u32(w, fields.len() as u32)?;
            for (name, val) in fields {
                write_str(w, name)?;
                write_value(w, val)?;
            }
        }
        Value::Shared { id, payload } => {
            write_u8(w, TAG_SHARED)?;
            write_u32(w, *id)?;
            write_value(w, payload)?;
        }
        Value::Ref { id } => {
            write_u8(w, TAG_REF)?;
            write_u32(w, *id)?;
        }
        Value::Weak { id } => {
            write_u8(w, TAG_WEAK)?;
            match id {
                None => write_u8(w, 0)?,
                Some(i) => {
                    write_u8(w, 1)?;
                    write_u32(w, *i)?;
                }
            }
        }
    }
    Ok(())
}

pub fn read_value<R: Read>(r: &mut R) -> Result<Value> {
    let tag = read_u8(r)?;
    Ok(match tag {
        TAG_I8 => {
            let mut b = [0u8; 1];
            read_exact(r, &mut b)?;
            Value::I8(i8::from_le_bytes(b))
        }
        TAG_I16 => {
            let mut b = [0u8; 2];
            read_exact(r, &mut b)?;
            Value::I16(i16::from_le_bytes(b))
        }
        TAG_I32 => {
            let mut b = [0u8; 4];
            read_exact(r, &mut b)?;
            Value::I32(i32::from_le_bytes(b))
        }
        TAG_I64 => {
            let mut b = [0u8; 8];
            read_exact(r, &mut b)?;
            Value::I64(i64::from_le_bytes(b))
        }
        TAG_U8 => Value::U8(read_u8(r)?),
        TAG_U16 => {
            let mut b = [0u8; 2];
            read_exact(r, &mut b)?;
            Value::U16(u16::from_le_bytes(b))
        }
        TAG_U32 => Value::U32(read_u32(r)?),
        TAG_U64 => Value::U64(read_u64(r)?),
        TAG_F32 => {
            let mut b = [0u8; 4];
            read_exact(r, &mut b)?;
            Value::F32(f32::from_le_bytes(b))
        }
        TAG_F64 => {
            let mut b = [0u8; 8];
            read_exact(r, &mut b)?;
            Value::F64(f64::from_le_bytes(b))
        }
        TAG_BOOL => Value::Bool(read_u8(r)? != 0),
        TAG_NULL => Value::Null,
        TAG_BYTES => {
            let n = read_u32(r)? as usize;
            let mut v = vec![0u8; n];
            read_exact(r, &mut v)?;
            Value::Bytes(v)
        }
        TAG_STRING => Value::String(read_string(r)?),
        TAG_ARRAY => {
            let n = read_u32(r)? as usize;
            let mut items = Vec::with_capacity(n);
            for _ in 0..n {
                items.push(read_value(r)?);
            }
            Value::Array(items)
        }
        TAG_STRUCT => {
            let schema_id = read_u32(r)?;
            let n = read_u32(r)? as usize;
            let mut fields = Vec::with_capacity(n);
            for _ in 0..n {
                let name = read_string(r)?;
                let v = read_value(r)?;
                fields.push((name, v));
            }
            Value::Struct { schema_id, fields }
        }
        TAG_SHARED => {
            let id = read_u32(r)?;
            let payload = Box::new(read_value(r)?);
            Value::Shared { id, payload }
        }
        TAG_REF => {
            let id = read_u32(r)?;
            Value::Ref { id }
        }
        TAG_WEAK => match read_u8(r)? {
            0 => Value::Weak { id: None },
            1 => {
                let id = read_u32(r)?;
                Value::Weak { id: Some(id) }
            }
            b => return Err(Error::UnknownTag(b)),
        },
        other => return Err(Error::UnknownTag(other)),
    })
}

#[derive(Debug, Clone)]
pub struct Entry {
    pub seq: u32,
    pub receiver_in: Option<Value>,
    pub receiver_out: Option<Value>,
    pub inputs: Vec<(String, Value)>,
    pub outcome: Outcome,
    pub content_hash: u64,
}

pub fn write_entry_body<W: Write>(w: &mut W, e: &Entry) -> Result<()> {
    write_u32(w, e.seq)?;

    let mut flags: u8 = 0;
    if e.receiver_in.is_some() {
        flags |= FLAG_HAS_RECEIVER_IN;
    }
    if e.receiver_out.is_some() {
        flags |= FLAG_HAS_RECEIVER_OUT;
    }
    write_u8(w, flags)?;

    write_u32(w, e.inputs.len() as u32)?;
    for (name, val) in &e.inputs {
        write_str(w, name)?;
        write_value(w, val)?;
    }

    if let Some(v) = &e.receiver_in {
        write_value(w, v)?;
    }
    if let Some(v) = &e.receiver_out {
        write_value(w, v)?;
    }

    match &e.outcome {
        Outcome::Return(outputs) => {
            write_u8(w, OUTCOME_RETURN)?;
            write_u32(w, outputs.len() as u32)?;
            for (name, val) in outputs {
                write_str(w, name)?;
                write_value(w, val)?;
            }
        }
        Outcome::Exception {
            type_name,
            what,
            payload,
        } => {
            write_u8(w, OUTCOME_EXCEPTION)?;
            write_str(w, type_name)?;
            write_str(w, what)?;
            match payload {
                None => write_u8(w, 0)?,
                Some(v) => {
                    write_u8(w, 1)?;
                    write_value(w, v)?;
                }
            }
        }
    }

    write_u64(w, e.content_hash)?;
    Ok(())
}

pub fn read_entry_body<R: Read>(r: &mut R) -> Result<Entry> {
    let seq = read_u32(r)?;
    let flags = read_u8(r)?;

    let input_count = read_u32(r)? as usize;
    let mut inputs = Vec::with_capacity(input_count);
    for _ in 0..input_count {
        let name = read_string(r)?;
        let v = read_value(r)?;
        inputs.push((name, v));
    }

    let receiver_in = if flags & FLAG_HAS_RECEIVER_IN != 0 {
        Some(read_value(r)?)
    } else {
        None
    };
    let receiver_out = if flags & FLAG_HAS_RECEIVER_OUT != 0 {
        Some(read_value(r)?)
    } else {
        None
    };

    let outcome_kind = read_u8(r)?;
    let outcome = match outcome_kind {
        OUTCOME_RETURN => {
            let n = read_u32(r)? as usize;
            let mut outputs = Vec::with_capacity(n);
            for _ in 0..n {
                let name = read_string(r)?;
                let v = read_value(r)?;
                outputs.push((name, v));
            }
            Outcome::Return(outputs)
        }
        OUTCOME_EXCEPTION => {
            let type_name = read_string(r)?;
            let what = read_string(r)?;
            let payload = match read_u8(r)? {
                0 => None,
                1 => Some(read_value(r)?),
                b => return Err(Error::UnknownTag(b)),
            };
            Outcome::Exception {
                type_name,
                what,
                payload,
            }
        }
        b => return Err(Error::UnknownTag(b)),
    };

    let content_hash = read_u64(r)?;

    Ok(Entry {
        seq,
        receiver_in,
        receiver_out,
        inputs,
        outcome,
        content_hash,
    })
}

pub fn write_file_prefix<W: Write>(w: &mut W, header: &LogHeader) -> Result<()> {
    w.write_all(&MAGIC)?;
    write_u32(w, SPEC_VERSION)?;
    let json = serde_json::to_vec(header)?;
    write_u32(w, json.len() as u32)?;
    w.write_all(&json)?;
    Ok(())
}

pub fn read_file_prefix<R: Read>(r: &mut R) -> Result<LogHeader> {
    let mut magic = [0u8; 4];
    read_exact(r, &mut magic)?;
    if magic != MAGIC {
        return Err(Error::BadMagic(magic));
    }
    let version = read_u32(r)?;
    if version != SPEC_VERSION {
        return Err(Error::UnsupportedVersion(version, SPEC_VERSION));
    }
    let header_len = read_u32(r)? as usize;
    let mut buf = vec![0u8; header_len];
    read_exact(r, &mut buf)?;
    let header: LogHeader = serde_json::from_slice(&buf)?;
    Ok(header)
}

pub fn write_framed_entry<W: Write>(w: &mut W, e: &Entry) -> Result<()> {
    let mut buf = Vec::with_capacity(128);
    write_entry_body(&mut buf, e)?;
    write_u32(w, buf.len() as u32)?;
    w.write_all(&buf)?;
    Ok(())
}

pub fn read_framed_entry<R: Read>(r: &mut R) -> Result<Option<Entry>> {
    let mut len_bytes = [0u8; 4];
    match r.read(&mut len_bytes)? {
        0 => return Ok(None),
        4 => {}
        n => {
            read_exact(r, &mut len_bytes[n..])?;
        }
    }
    let len = u32::from_le_bytes(len_bytes) as usize;
    let mut buf = vec![0u8; len];
    read_exact(r, &mut buf)?;
    let e = read_entry_body(&mut buf.as_slice())?;
    Ok(Some(e))
}

/// Serialize `inputs` (including aliasing shape) into a canonical byte stream
/// for content hashing.
pub fn canonical_input_bytes(
    receiver_in: Option<&Value>,
    inputs: &[(String, Value)],
) -> Result<Vec<u8>> {
    let mut out = Vec::with_capacity(128);
    match receiver_in {
        None => write_u8(&mut out, 0)?,
        Some(v) => {
            write_u8(&mut out, 1)?;
            write_value(&mut out, v)?;
        }
    }
    write_u32(&mut out, inputs.len() as u32)?;
    for (name, val) in inputs {
        write_str(&mut out, name)?;
        write_value(&mut out, val)?;
    }
    Ok(out)
}
