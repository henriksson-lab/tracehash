//! The canonical `Value` enum: wire-level type system for dclog entries.

use super::error::{Error, Result};

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    I8(i8),
    I16(i16),
    I32(i32),
    I64(i64),
    U8(u8),
    U16(u16),
    U32(u32),
    U64(u64),
    F32(f32),
    F64(f64),
    Bool(bool),
    Null,
    Bytes(Vec<u8>),
    String(String),
    Array(Vec<Value>),
    Struct {
        schema_id: u32,
        fields: Vec<(String, Value)>,
    },
    Shared {
        id: u32,
        payload: Box<Value>,
    },
    Ref {
        id: u32,
    },
    Weak {
        id: Option<u32>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Return(Vec<(String, Value)>),
    Exception {
        type_name: String,
        what: String,
        payload: Option<Value>,
    },
}

impl Value {
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::I8(_) => "i8",
            Value::I16(_) => "i16",
            Value::I32(_) => "i32",
            Value::I64(_) => "i64",
            Value::U8(_) => "u8",
            Value::U16(_) => "u16",
            Value::U32(_) => "u32",
            Value::U64(_) => "u64",
            Value::F32(_) => "f32",
            Value::F64(_) => "f64",
            Value::Bool(_) => "bool",
            Value::Null => "null",
            Value::Bytes(_) => "bytes",
            Value::String(_) => "string",
            Value::Array(_) => "array",
            Value::Struct { .. } => "struct",
            Value::Shared { .. } => "shared",
            Value::Ref { .. } => "ref",
            Value::Weak { .. } => "weak",
        }
    }

    pub fn as_i8(&self) -> Result<i8> {
        if let Value::I8(v) = self {
            Ok(*v)
        } else {
            Err(Self::shape("i8", self))
        }
    }
    pub fn as_i16(&self) -> Result<i16> {
        match self {
            Value::I16(v) => Ok(*v),
            Value::I8(v) => Ok(*v as i16),
            _ => Err(Self::shape("i16", self)),
        }
    }
    pub fn as_i32(&self) -> Result<i32> {
        match self {
            Value::I32(v) => Ok(*v),
            Value::I16(v) => Ok(*v as i32),
            Value::I8(v) => Ok(*v as i32),
            _ => Err(Self::shape("i32", self)),
        }
    }
    pub fn as_i64(&self) -> Result<i64> {
        match self {
            Value::I64(v) => Ok(*v),
            Value::I32(v) => Ok(*v as i64),
            Value::I16(v) => Ok(*v as i64),
            Value::I8(v) => Ok(*v as i64),
            _ => Err(Self::shape("i64", self)),
        }
    }

    pub fn as_u8(&self) -> Result<u8> {
        if let Value::U8(v) = self {
            Ok(*v)
        } else {
            Err(Self::shape("u8", self))
        }
    }
    pub fn as_u16(&self) -> Result<u16> {
        match self {
            Value::U16(v) => Ok(*v),
            Value::U8(v) => Ok(*v as u16),
            _ => Err(Self::shape("u16", self)),
        }
    }
    pub fn as_u32(&self) -> Result<u32> {
        match self {
            Value::U32(v) => Ok(*v),
            Value::U16(v) => Ok(*v as u32),
            Value::U8(v) => Ok(*v as u32),
            _ => Err(Self::shape("u32", self)),
        }
    }
    pub fn as_u64(&self) -> Result<u64> {
        match self {
            Value::U64(v) => Ok(*v),
            Value::U32(v) => Ok(*v as u64),
            Value::U16(v) => Ok(*v as u64),
            Value::U8(v) => Ok(*v as u64),
            _ => Err(Self::shape("u64", self)),
        }
    }

    pub fn as_f32(&self) -> Result<f32> {
        if let Value::F32(v) = self {
            Ok(*v)
        } else {
            Err(Self::shape("f32", self))
        }
    }

    pub fn as_f64(&self) -> Result<f64> {
        match self {
            Value::F64(v) => Ok(*v),
            Value::F32(v) => Ok(*v as f64),
            _ => Err(Self::shape("f64", self)),
        }
    }

    pub fn as_bool(&self) -> Result<bool> {
        if let Value::Bool(v) = self {
            Ok(*v)
        } else {
            Err(Self::shape("bool", self))
        }
    }

    pub fn as_str(&self) -> Result<&str> {
        if let Value::String(s) = self {
            Ok(s)
        } else {
            Err(Self::shape("string", self))
        }
    }

    pub fn as_bytes(&self) -> Result<&[u8]> {
        if let Value::Bytes(b) = self {
            Ok(b)
        } else {
            Err(Self::shape("bytes", self))
        }
    }

    pub fn as_array(&self) -> Result<&[Value]> {
        if let Value::Array(v) = self {
            Ok(v)
        } else {
            Err(Self::shape("array", self))
        }
    }

    pub fn as_struct(&self) -> Result<(u32, &[(String, Value)])> {
        if let Value::Struct { schema_id, fields } = self {
            Ok((*schema_id, fields))
        } else {
            Err(Self::shape("struct", self))
        }
    }

    pub fn field(&self, name: &str) -> Result<&Value> {
        let (_, fields) = self.as_struct()?;
        fields
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v)
            .ok_or_else(|| Error::MissingField(name.to_string()))
    }

    fn shape(expected: &'static str, got: &Value) -> Error {
        Error::ShapeMismatch {
            expected,
            actual: got.type_name().to_string(),
        }
    }
}
