//! Wire-format tag bytes. Keep in lockstep with `c/tracehash_c.c` dclog writer.

pub const TAG_I8: u8 = 0x01;
pub const TAG_I16: u8 = 0x02;
pub const TAG_I32: u8 = 0x03;
pub const TAG_I64: u8 = 0x04;
pub const TAG_U8: u8 = 0x05;
pub const TAG_U16: u8 = 0x06;
pub const TAG_U32: u8 = 0x07;
pub const TAG_U64: u8 = 0x08;
pub const TAG_F32: u8 = 0x09;
pub const TAG_F64: u8 = 0x0A;
pub const TAG_BOOL: u8 = 0x0B;
pub const TAG_NULL: u8 = 0x0C;
pub const TAG_BYTES: u8 = 0x0D;
pub const TAG_STRING: u8 = 0x0E;
pub const TAG_ARRAY: u8 = 0x0F;
pub const TAG_STRUCT: u8 = 0x10;
pub const TAG_SHARED: u8 = 0x20;
pub const TAG_REF: u8 = 0x21;
pub const TAG_WEAK: u8 = 0x22;

pub const OUTCOME_RETURN: u8 = 0x00;
pub const OUTCOME_EXCEPTION: u8 = 0x01;

pub const FLAG_HAS_RECEIVER_IN: u8 = 0x01;
pub const FLAG_HAS_RECEIVER_OUT: u8 = 0x02;
