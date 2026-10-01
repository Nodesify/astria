// packstream: Bolt's PackStream 2 wire format — the subset the client
// needs. Encoding covers every value the push sends; decoding covers the
// response shapes servers return (success/failure metadata maps). Spec:
// https://neo4j.com/docs/bolt/current/packstream/

use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Boolean(bool),
    Integer(i64),
    Float(f64),
    String(String),
    List(Vec<Value>),
    /// BTreeMap: field order is deterministic, which keeps tests (and
    /// reproducible pushes) honest.
    Map(BTreeMap<String, Value>),
}

/// One-byte markers for tiny forms; named so the encoder reads like the spec.
mod marker {
    pub const TINY_STRING: u8 = 0x80; // + (size < 0x10)
    pub const TINY_LIST: u8 = 0x90; // + (size < 0x10)
    pub const TINY_MAP: u8 = 0xA0; // + (size < 0x10)
    pub const TINY_STRUCT: u8 = 0xB0; // + (size < 0x10)
    pub const NULL: u8 = 0xC0;
    pub const FLOAT64: u8 = 0xC1;
    pub const FALSE: u8 = 0xC2;
    pub const TRUE: u8 = 0xC3;
    pub const INT8: u8 = 0xC8;
    pub const INT16: u8 = 0xC9;
    pub const INT32: u8 = 0xCA;
    pub const INT64: u8 = 0xCB;
    pub const STRING_8: u8 = 0xD0;
    pub const STRING_16: u8 = 0xD1;
    pub const STRING_32: u8 = 0xD2;
    pub const LIST_8: u8 = 0xD4;
    pub const LIST_16: u8 = 0xD5;
    pub const LIST_32: u8 = 0xD6;
    pub const MAP_8: u8 = 0xD8;
    pub const MAP_16: u8 = 0xD9;
    pub const MAP_32: u8 = 0xDA;
    pub const STRUCT_8: u8 = 0xDC;
    pub const STRUCT_16: u8 = 0xDD;
}

fn push_size(
    buf: &mut Vec<u8>,
    marker_tiny: u8,
    marker8: u8,
    marker16: u8,
    marker32: u8,
    size: usize,
) {
    match size {
        s if s < 0x10 => buf.push(marker_tiny + s as u8),
        s if s < 0x100 => {
            buf.push(marker8);
            buf.push(s as u8);
        }
        s if s < 0x1_0000 => {
            buf.push(marker16);
            buf.extend_from_slice(&(s as u16).to_be_bytes());
        }
        s => {
            // PackStream has no TINY/8/16 variant above 16-bit for strings
            // other than the 32-bit form.
            buf.push(marker32);
            buf.extend_from_slice(&(s as u32).to_be_bytes());
        }
    }
}

pub fn encode(value: &Value, buf: &mut Vec<u8>) {
    match value {
        Value::Null => buf.push(marker::NULL),
        Value::Boolean(true) => buf.push(marker::TRUE),
        Value::Boolean(false) => buf.push(marker::FALSE),
        Value::Integer(n) => encode_int(*n, buf),
        Value::Float(f) => {
            buf.push(marker::FLOAT64);
            buf.extend_from_slice(&f.to_be_bytes());
        }
        Value::String(s) => {
            push_size(
                buf,
                marker::TINY_STRING,
                marker::STRING_8,
                marker::STRING_16,
                marker::STRING_32,
                s.len(),
            );
            buf.extend_from_slice(s.as_bytes());
        }
        Value::List(items) => {
            push_size(
                buf,
                marker::TINY_LIST,
                marker::LIST_8,
                marker::LIST_16,
                marker::LIST_32,
                items.len(),
            );
            for item in items {
                encode(item, buf);
            }
        }
        Value::Map(map) => {
            push_size(
                buf,
                marker::TINY_MAP,
                marker::MAP_8,
                marker::MAP_16,
                marker::MAP_32,
                map.len(),
            );
            for (key, val) in map {
                encode(&Value::String(key.clone()), buf);
                encode(val, buf);
            }
        }
    }
}

/// Ints use the smallest representation: in-range values ride the marker
/// byte itself (tiny ints), then INT8/16/32/64.
fn encode_int(n: i64, buf: &mut Vec<u8>) {
    if (-16..=127).contains(&n) {
        buf.push(n as i8 as u8);
    } else if (-128..=127).contains(&n) {
        buf.push(marker::INT8);
        buf.push(n as i8 as u8);
    } else if (-32768..=32767).contains(&n) {
        buf.push(marker::INT16);
        buf.extend_from_slice(&(n as i16).to_be_bytes());
    } else if (-2_147_483_648..=2_147_483_647).contains(&n) {
        buf.push(marker::INT32);
        buf.extend_from_slice(&(n as i32).to_be_bytes());
    } else {
        buf.push(marker::INT64);
        buf.extend_from_slice(&n.to_be_bytes());
    }
}

/// A decoded struct: Bolt messages and metadata structures.
#[derive(Debug, Clone, PartialEq)]
pub struct Struct {
    pub tag: u8,
    pub fields: Vec<Value>,
}

pub fn encode_struct(tag: u8, fields: &[Value], buf: &mut Vec<u8>) {
    push_size(
        buf,
        marker::TINY_STRUCT,
        marker::STRUCT_8,
        marker::STRUCT_16,
        // No 32-bit struct form exists in the spec; sizes above 65_535 are
        // not expressible and not needed (messages have few fields).
        marker::STRUCT_16,
        fields.len(),
    );
    buf.push(tag);
    for field in fields {
        encode(field, buf);
    }
}

// ---------------------------------------------------------------------------
// Decoding (responses only)
// ---------------------------------------------------------------------------

/// Decode one complete PackStream value from the front of `buf`, returning
/// it and the number of bytes consumed.
pub fn decode(buf: &[u8]) -> Result<(Value, usize), String> {
    if buf.is_empty() {
        return Err("packstream: empty buffer".into());
    }
    let marker = buf[0];
    let (mut pos, size): (usize, usize) = match marker {
        0x80..=0x8F => (1, (marker & 0x0F) as usize),
        0xD0 => (2, *buf.get(1).ok_or("truncated STRING_8")? as usize),
        0xD1 => (3, u16::from_be_bytes(slice2(buf, 1)?) as usize),
        0xD2 => (5, u32::from_be_bytes(slice4(buf, 1)?) as usize),
        0x90..=0x9F => (1, (marker & 0x0F) as usize),
        0xD4 => (2, *buf.get(1).ok_or("truncated LIST_8")? as usize),
        0xD5 => (3, u16::from_be_bytes(slice2(buf, 1)?) as usize),
        0xD6 => (5, u32::from_be_bytes(slice4(buf, 1)?) as usize),
        0xA0..=0xAF => (1, (marker & 0x0F) as usize),
        0xD8 => (2, *buf.get(1).ok_or("truncated MAP_8")? as usize),
        0xD9 => (3, u16::from_be_bytes(slice2(buf, 1)?) as usize),
        0xDA => (5, u32::from_be_bytes(slice4(buf, 1)?) as usize),
        0xB0..=0xBF => (2, (marker & 0x0F) as usize),
        0xDC => (2, *buf.get(1).ok_or("truncated STRUCT_8")? as usize),
        0xDD => (3, u16::from_be_bytes(slice2(buf, 1)?) as usize),
        0xC0 | 0xC2 | 0xC3 => (1, 0),
        0xC1 => (9, 0),
        // Tiny ints ride the marker byte: 0x00..=0x7F are +0..=127,
        // 0xF0..=0xFF are -16..=-1.
        0x00..=0x7F => (1, 0),
        0xF0..=0xFF => (1, 0),
        0xC8 => (2, 0),
        0xC9 => (3, 0),
        0xCA => (5, 0),
        0xCB => (9, 0),
        other => return Err(format!("packstream: unsupported marker 0x{other:02X}")),
    };

    match marker {
        0x80..=0x8F | 0xD0 | 0xD1 | 0xD2 => {
            let end = pos + size;
            let bytes = buf.get(pos..end).ok_or("packstream: truncated string")?;
            Ok((
                Value::String(String::from_utf8_lossy(bytes).into_owned()),
                end,
            ))
        }
        0x90..=0x9F | 0xD4 | 0xD5 | 0xD6 => {
            // Every element consumes at least one byte, so a real list never
            // has more elements than the buffer has left. Bounding the
            // capacity by that turns a hostile LIST_32 length into the
            // ordinary "truncated" error below instead of a giant allocation.
            let mut items = Vec::with_capacity(size.min(decode_capacity(buf, pos)));
            for _ in 0..size {
                let (value, used) = decode(&buf[pos..])?;
                pos += used;
                items.push(value);
            }
            Ok((Value::List(items), pos))
        }
        0xA0..=0xAF | 0xD8 | 0xD9 | 0xDA => {
            let mut map = BTreeMap::new();
            for _ in 0..size {
                let (Value::String(key), used) = decode(&buf[pos..])? else {
                    return Err("packstream: map key must be a string".into());
                };
                pos += used;
                let (value, used) = decode(&buf[pos..])?;
                pos += used;
                map.insert(key, value);
            }
            Ok((Value::Map(map), pos))
        }
        0xB0..=0xBF | 0xDC | 0xDD => {
            // The tag byte (position 1) is read by `decode_struct`; here we
            // only surface the field values.
            pos = 2;
            // Same server-length bound as the list arm above.
            let mut fields = Vec::with_capacity(size.min(decode_capacity(buf, pos)));
            for _ in 0..size {
                let (value, used) = decode(&buf[pos..])?;
                pos += used;
                fields.push(value);
            }
            // Structs surface as their fields wrapped in a List tagged by
            // the caller when needed; the client only matches on the tag
            // byte, so a plain List keeps the Value enum small.
            Ok((Value::List(fields), pos))
        }
        0xC0 => Ok((Value::Null, 1)),
        0xC2 => Ok((Value::Boolean(false), 1)),
        0xC3 => Ok((Value::Boolean(true), 1)),
        0xC1 => Ok((Value::Float(f64::from_be_bytes(slice8(buf, 1)?)), 9)),
        0x00..=0x7F => Ok((Value::Integer(marker as i64), 1)),
        0xF0..=0xFF => Ok((Value::Integer(marker as i8 as i64), 1)),
        0xC8 => Ok((Value::Integer(buf[1] as i8 as i64), 2)),
        0xC9 => Ok((
            Value::Integer(i16::from_be_bytes(slice2(buf, 1)?) as i64),
            3,
        )),
        0xCA => Ok((
            Value::Integer(i32::from_be_bytes(slice4(buf, 1)?) as i64),
            5,
        )),
        0xCB => Ok((Value::Integer(i64::from_be_bytes(slice8(buf, 1)?)), 9)),
        // The match above already rejected everything else.
        _ => unreachable!(),
    }
}

/// Decode a struct's tag + fields (used to classify SUMMARY/RECORD frames).
pub fn decode_struct(buf: &[u8]) -> Result<(u8, Vec<Value>, usize), String> {
    let marker = *buf.first().ok_or("packstream: empty buffer")?;
    if !(0xB0..=0xBF).contains(&marker) && marker != 0xDC && marker != 0xDD {
        return Err("packstream: not a struct".into());
    }
    let size = match marker {
        0xB0..=0xBF => (marker & 0x0F) as usize,
        0xDC => *buf.get(1).ok_or("truncated STRUCT_8")? as usize,
        _ => u16::from_be_bytes(slice2(buf, 1)?) as usize,
    };
    let header = if (0xB0..=0xBF).contains(&marker) {
        2
    } else if marker == 0xDC {
        3
    } else {
        4
    };
    let tag = *buf
        .get(header - 1)
        .ok_or("packstream: missing struct tag")?;
    let mut pos = header;
    // Same server-length bound as decode's list arm: a declared field count
    // beyond the remaining bytes is truncation, not a reason to allocate.
    let mut fields = Vec::with_capacity(size.min(decode_capacity(buf, pos)));
    for _ in 0..size {
        let (value, used) = decode(&buf[pos..])?;
        pos += used;
        fields.push(value);
    }
    Ok((tag, fields, pos))
}

/// Upper bound on a container's element count given a buffer of `len`
/// remaining bytes: each encoded element (or map key) costs at least one
/// byte, so a well-formed message can never exceed this. Used to clamp
/// server-supplied sizes before any allocation.
fn decode_capacity(buf: &[u8], pos: usize) -> usize {
    buf.len().saturating_sub(pos)
}

fn slice2(buf: &[u8], at: usize) -> Result<[u8; 2], String> {
    buf.get(at..at + 2)
        .map(|s| [s[0], s[1]])
        .ok_or_else(|| "packstream: truncated".into())
}
fn slice4(buf: &[u8], at: usize) -> Result<[u8; 4], String> {
    buf.get(at..at + 4)
        .map(|s| [s[0], s[1], s[2], s[3]])
        .ok_or_else(|| "packstream: truncated".into())
}
fn slice8(buf: &[u8], at: usize) -> Result<[u8; 8], String> {
    buf.get(at..at + 8)
        .map(|s| {
            let mut out = [0u8; 8];
            out.copy_from_slice(s);
            out
        })
        .ok_or_else(|| "packstream: truncated".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hostile_list_length_is_truncation_not_allocation() {
        // LIST_32 declaring ~4 billion elements in a 5-byte buffer: before
        // the capacity bound this was a Vec::with_capacity(0xFFFF_FFFF)
        // abort on the allocator; it must surface as an ordinary error.
        let buf = [0xD6u8, 0xFF, 0xFF, 0xFF, 0xFF];
        let result = decode(&buf);
        assert!(result.is_err(), "oversized list must error, got {result:?}");
    }

    #[test]
    fn hostile_map_length_is_truncation_not_allocation() {
        // MAP_32 declaring 4 billion entries in 5 bytes; map decode does not
        // preallocate, but the key loop must terminate with an error rather
        // than spin on an exhausted buffer.
        let buf = [0xDAu8, 0xFF, 0xFF, 0xFF, 0xFF];
        let result = decode(&buf);
        assert!(result.is_err());
    }

    #[test]
    fn hostile_struct_field_count_is_truncation_not_allocation() {
        // STRUCT_8 declaring 200 fields but only a tag byte present.
        let buf = [0xDCu8, 0xC8, 0x70];
        let result = decode_struct(&buf);
        assert!(result.is_err());
    }

    fn enc(value: &Value) -> Vec<u8> {
        let mut buf = Vec::new();
        encode(value, &mut buf);
        buf
    }

    #[test]
    fn tiny_forms_use_marker_byte() {
        assert_eq!(enc(&Value::Integer(5)), vec![0x05]);
        assert_eq!(enc(&Value::Integer(-9)), vec![0xF7]);
        assert_eq!(enc(&Value::String("ab".into())), vec![0x82, b'a', b'b']);
        assert_eq!(enc(&Value::Null), vec![0xC0]);
        assert_eq!(enc(&Value::Boolean(true)), vec![0xC3]);
    }

    #[test]
    fn int_widths_shrink_and_grow_correctly() {
        // 200 does not fit signed INT8 - it takes the INT16 form.
        assert_eq!(enc(&Value::Integer(200)), vec![0xC9, 0x00, 0xC8]);
        assert_eq!(enc(&Value::Integer(-100)), vec![0xC8, 0x9C]);
        assert_eq!(enc(&Value::Integer(1000)), vec![0xC9, 0x03, 0xE8]);
        assert_eq!(
            enc(&Value::Integer(100_000)),
            vec![0xCA, 0x00, 0x01, 0x86, 0xA0]
        );
        assert_eq!(
            enc(&Value::Integer(8_589_934_592)),
            vec![0xCB, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00]
        );
    }

    #[test]
    fn roundtrip_mixed_map() {
        let mut map = BTreeMap::new();
        map.insert("name".to_string(), Value::String("login()".into()));
        map.insert("degree".to_string(), Value::Integer(42));
        map.insert("score".to_string(), Value::Float(0.5));
        map.insert(
            "flags".to_string(),
            Value::List(vec![Value::Boolean(true), Value::Null]),
        );
        let bytes = enc(&Value::Map(map.clone()));
        let (decoded, used) = decode(&bytes).unwrap();
        assert_eq!(used, bytes.len());
        assert_eq!(decoded, Value::Map(map));
    }

    #[test]
    fn struct_encode_decode_preserves_tag() {
        let mut buf = Vec::new();
        encode_struct(
            0x10,
            &[Value::String("q".into()), Value::Map(Default::default())],
            &mut buf,
        );
        let (tag, fields, used) = decode_struct(&buf).unwrap();
        assert_eq!(tag, 0x10);
        assert_eq!(fields.len(), 2);
        assert_eq!(used, buf.len());
    }

    #[test]
    fn long_strings_take_the_16bit_form() {
        let s = "x".repeat(300);
        let bytes = enc(&Value::String(s.clone()));
        assert_eq!(bytes[0], 0xD1);
        let (decoded, used) = decode(&bytes).unwrap();
        assert_eq!(decoded, Value::String(s));
        assert_eq!(used, bytes.len());
    }
}
