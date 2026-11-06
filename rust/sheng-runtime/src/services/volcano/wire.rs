//! Minimal protobuf wire format: varints and length-delimited fields only.
//!
//! Only what the AST v2 messages need. Groups (wire types 3 and 4) and unknown wire types
//! are protocol errors; 32- and 64-bit fixed fields are accepted so that unknown fields of
//! those types can be skipped.

use thiserror::Error;

/// Largest single length-delimited field accepted from the service.
pub const MAX_FIELD_BYTES: usize = 1 << 20;
/// Largest whole message accepted from the service.
pub const MAX_MESSAGE_BYTES: usize = 2 << 20;

const VARINT_MAX_BYTES: usize = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum WireError {
    #[error("message ends inside a field")]
    Truncated,
    #[error("varint is longer than ten bytes")]
    VarintOverflow,
    #[error("unsupported wire type")]
    WireType,
    #[error("field number is zero or out of range")]
    FieldNumber,
    #[error("field or message is longer than the limit")]
    TooLong,
    #[error("single-value field appears more than once")]
    Duplicate,
    #[error("string field is not UTF-8")]
    Utf8,
}

/// Value of one decoded field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Value<'a> {
    Varint(u64),
    Fixed64(u64),
    Bytes(&'a [u8]),
    Fixed32(u32),
}

impl<'a> Value<'a> {
    pub fn varint(self) -> Result<u64, WireError> {
        match self {
            Self::Varint(value) => Ok(value),
            _ => Err(WireError::WireType),
        }
    }

    /// proto3 `int32`: the low 32 bits of the varint, two's complement.
    pub fn int32(self) -> Result<i32, WireError> {
        self.varint().map(|value| value as u32 as i32)
    }

    pub fn bytes(self) -> Result<&'a [u8], WireError> {
        match self {
            Self::Bytes(bytes) => Ok(bytes),
            _ => Err(WireError::WireType),
        }
    }

    pub fn string(self) -> Result<&'a str, WireError> {
        std::str::from_utf8(self.bytes()?).map_err(|_| WireError::Utf8)
    }
}

/// Iterates over the fields of one message.
pub struct Reader<'a> {
    buf: &'a [u8],
}

impl<'a> Reader<'a> {
    pub fn new(buf: &'a [u8]) -> Result<Self, WireError> {
        if buf.len() > MAX_MESSAGE_BYTES {
            return Err(WireError::TooLong);
        }
        Ok(Self { buf })
    }

    /// Next `(field number, value)`, or `None` at the end of the message.
    pub fn next_field(&mut self) -> Result<Option<(u32, Value<'a>)>, WireError> {
        if self.buf.is_empty() {
            return Ok(None);
        }
        let key = self.varint()?;
        let number = u32::try_from(key >> 3).map_err(|_| WireError::FieldNumber)?;
        if number == 0 || number > (1 << 29) - 1 {
            return Err(WireError::FieldNumber);
        }
        let value = match key & 0x7 {
            0 => Value::Varint(self.varint()?),
            1 => Value::Fixed64(u64::from_le_bytes(self.take_array()?)),
            2 => {
                let len = usize::try_from(self.varint()?).map_err(|_| WireError::TooLong)?;
                if len > MAX_FIELD_BYTES {
                    return Err(WireError::TooLong);
                }
                Value::Bytes(self.take(len)?)
            }
            5 => Value::Fixed32(u32::from_le_bytes(self.take_array()?)),
            _ => return Err(WireError::WireType),
        };
        Ok(Some((number, value)))
    }

    fn varint(&mut self) -> Result<u64, WireError> {
        let mut value = 0u64;
        for index in 0..VARINT_MAX_BYTES {
            let (&byte, rest) = self.buf.split_first().ok_or(WireError::Truncated)?;
            self.buf = rest;
            let low = u64::from(byte & 0x7f);
            if index == VARINT_MAX_BYTES - 1 && low > 1 {
                return Err(WireError::VarintOverflow);
            }
            value |= low << (7 * index);
            if byte & 0x80 == 0 {
                return Ok(value);
            }
        }
        Err(WireError::VarintOverflow)
    }

    fn take(&mut self, len: usize) -> Result<&'a [u8], WireError> {
        if len > self.buf.len() {
            return Err(WireError::Truncated);
        }
        let (head, rest) = self.buf.split_at(len);
        self.buf = rest;
        Ok(head)
    }

    fn take_array<const N: usize>(&mut self) -> Result<[u8; N], WireError> {
        let bytes = self.take(N)?;
        let mut array = [0u8; N];
        array.copy_from_slice(bytes);
        Ok(array)
    }
}

/// Stores a single-value field, rejecting a second occurrence.
pub fn set_once<T>(slot: &mut Option<T>, value: T) -> Result<(), WireError> {
    if slot.is_some() {
        return Err(WireError::Duplicate);
    }
    *slot = Some(value);
    Ok(())
}

/// Builds one message. Default values (0, empty) are omitted as in proto3.
#[derive(Debug, Default)]
pub struct Writer {
    buf: Vec<u8>,
}

impl Writer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn varint(mut self, number: u32, value: u64) -> Self {
        if value != 0 {
            self.key(number, 0);
            put_varint(&mut self.buf, value);
        }
        self
    }

    /// proto3 `int32`: negative values are sign-extended to ten bytes.
    pub fn int32(self, number: u32, value: i32) -> Self {
        self.varint(number, i64::from(value) as u64)
    }

    pub fn bytes(mut self, number: u32, value: &[u8]) -> Self {
        if !value.is_empty() {
            self.key(number, 2);
            put_varint(&mut self.buf, value.len() as u64);
            self.buf.extend_from_slice(value);
        }
        self
    }

    pub fn string(self, number: u32, value: &str) -> Self {
        self.bytes(number, value.as_bytes())
    }

    /// Embedded message; written even when empty so that its presence is visible.
    pub fn message(mut self, number: u32, inner: Writer) -> Self {
        self.key(number, 2);
        put_varint(&mut self.buf, inner.buf.len() as u64);
        self.buf.extend_from_slice(&inner.buf);
        self
    }

    pub fn finish(self) -> Vec<u8> {
        self.buf
    }

    fn key(&mut self, number: u32, wire_type: u8) {
        put_varint(
            &mut self.buf,
            (u64::from(number) << 3) | u64::from(wire_type),
        );
    }
}

pub fn put_varint(buf: &mut Vec<u8>, mut value: u64) {
    while value >= 0x80 {
        buf.push((value as u8) | 0x80);
        value >>= 7;
    }
    buf.push(value as u8);
}
