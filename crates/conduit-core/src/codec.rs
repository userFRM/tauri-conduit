//! Binary frame format and wire encoding traits for conduit.
//!
//! Every conduit message is framed with an 11-byte header followed by
//! a variable-length payload. The [`Encode`] / [`Decode`] traits
//! provide zero-copy-friendly serialisation for primitive types, byte
//! vectors, and strings.
//!
//! # Frame layout (11 bytes)
//!
//! | Offset | Size | Field            | Notes                                  |
//! |--------|------|------------------|----------------------------------------|
//! | 0      | 1    | `version`        | Always [`PROTOCOL_VERSION`] (1)        |
//! | 1      | 1    | `reserved` | 0=protocol (reserved for future use)   |
//! | 2      | 1    | `msg_type`       | See [`MsgType`]                        |
//! | 3      | 4    | `sequence`       | LE u32, monotonic counter              |
//! | 7      | 4    | `payload_len`    | LE u32, byte length of trailing data   |

/// Size of the binary frame header in bytes.
pub const FRAME_HEADER_SIZE: usize = 11;

/// Current protocol version written into every frame.
pub const PROTOCOL_VERSION: u8 = 1;

// ---------------------------------------------------------------------------
// MsgType
// ---------------------------------------------------------------------------

/// Message-type tag carried in the frame header.
///
/// Known variants cover the core protocol; user-defined types start at `0x10`.
/// Any `u8` value is accepted on the wire via [`MsgType::Other`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MsgType {
    /// Client-to-server request (`0x00`).
    Request,
    /// Server-to-client response (`0x01`).
    Response,
    /// Server push / event (`0x02`).
    Push,
    /// Error frame (`0x04`).
    Error,
    /// Any other message type (user-defined, `0x10`+).
    Other(u8),
}

impl MsgType {
    /// Convert from the on-wire `u8` representation.
    #[inline]
    pub fn from_u8(v: u8) -> Self {
        match v {
            0x00 => Self::Request,
            0x01 => Self::Response,
            0x02 => Self::Push,
            0x04 => Self::Error,
            other => Self::Other(other),
        }
    }

    /// Convert to the on-wire `u8` representation.
    #[inline]
    pub fn to_u8(self) -> u8 {
        match self {
            Self::Request => 0x00,
            Self::Response => 0x01,
            Self::Push => 0x02,
            Self::Error => 0x04,
            Self::Other(v) => v,
        }
    }
}

// ---------------------------------------------------------------------------
// FrameHeader
// ---------------------------------------------------------------------------

/// Parsed representation of the 11-byte frame header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameHeader {
    /// Protocol version (always [`PROTOCOL_VERSION`]).
    pub version: u8,
    /// Transport identifier: 0=protocol (reserved for future use).
    pub reserved: u8,
    /// Message type tag.
    pub msg_type: MsgType,
    /// Monotonically increasing sequence number (LE).
    pub sequence: u32,
    /// Length of the payload that follows this header (LE).
    pub payload_len: u32,
}

impl FrameHeader {
    /// Serialise the header into `buf` (appends exactly [`FRAME_HEADER_SIZE`] bytes).
    #[inline]
    pub fn write_to(&self, buf: &mut Vec<u8>) {
        buf.push(self.version);
        buf.push(self.reserved);
        buf.push(self.msg_type.to_u8());
        buf.extend_from_slice(&self.sequence.to_le_bytes());
        buf.extend_from_slice(&self.payload_len.to_le_bytes());
    }

    /// Attempt to parse a header from the first 11 bytes of `data`.
    ///
    /// Returns `None` if `data` is shorter than [`FRAME_HEADER_SIZE`].
    #[inline]
    pub fn read_from(data: &[u8]) -> Option<Self> {
        if data.len() < FRAME_HEADER_SIZE {
            return None;
        }
        let version = data[0];
        let reserved = data[1];
        let msg_type = MsgType::from_u8(data[2]);
        let sequence = u32::from_le_bytes([data[3], data[4], data[5], data[6]]);
        let payload_len = u32::from_le_bytes([data[7], data[8], data[9], data[10]]);
        Some(Self {
            version,
            reserved,
            msg_type,
            sequence,
            payload_len,
        })
    }
}

// ---------------------------------------------------------------------------
// frame_pack / frame_unpack
// ---------------------------------------------------------------------------

/// Build a complete frame: header bytes followed by payload bytes.
#[inline]
#[must_use]
pub fn frame_pack(header: &FrameHeader, payload: &[u8]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(FRAME_HEADER_SIZE + payload.len());
    header.write_to(&mut buf);
    buf.extend_from_slice(payload);
    buf
}

/// Split a complete frame into its header and payload slice.
///
/// Returns `None` if the data is too short for the header, or if the
/// remaining bytes are fewer than `payload_len`.
#[inline]
#[must_use]
pub fn frame_unpack(data: &[u8]) -> Option<(FrameHeader, &[u8])> {
    let header = FrameHeader::read_from(data)?;
    let payload_end = FRAME_HEADER_SIZE + header.payload_len as usize;
    if data.len() < payload_end {
        return None;
    }
    Some((header, &data[FRAME_HEADER_SIZE..payload_end]))
}

// ---------------------------------------------------------------------------
// Encode / Decode traits
// ---------------------------------------------------------------------------

/// Encode a value into a byte buffer in conduit's binary wire format.
pub trait Encode {
    /// Append the encoded representation to `buf`.
    fn encode(&self, buf: &mut Vec<u8>);

    /// The exact number of bytes that [`encode`](Encode::encode)
    /// will append.
    fn encode_size(&self) -> usize;
}

/// Decode a value from a byte slice in conduit's binary wire format.
///
/// Returns the decoded value together with the number of bytes consumed,
/// or `None` if the data is too short or malformed.
pub trait Decode: Sized {
    /// Attempt to decode from the start of `data`.
    fn decode(data: &[u8]) -> Option<(Self, usize)>;
}

// ---------------------------------------------------------------------------
// Primitive impls
// ---------------------------------------------------------------------------

macro_rules! impl_wire_int {
    ($($ty:ty),+) => {
        $(
            impl Encode for $ty {
                fn encode(&self, buf: &mut Vec<u8>) {
                    buf.extend_from_slice(&self.to_le_bytes());
                }

                fn encode_size(&self) -> usize {
                    std::mem::size_of::<$ty>()
                }
            }

            impl Decode for $ty {
                fn decode(data: &[u8]) -> Option<(Self, usize)> {
                    const SIZE: usize = std::mem::size_of::<$ty>();
                    if data.len() < SIZE {
                        return None;
                    }
                    let arr: [u8; SIZE] = data[..SIZE].try_into().ok()?;
                    Some((<$ty>::from_le_bytes(arr), SIZE))
                }
            }
        )+
    };
}

impl_wire_int!(u8, u16, u32, u64, i8, i16, i32, i64, f32, f64);

// bool: encoded as a single byte (0 or 1).
impl Encode for bool {
    fn encode(&self, buf: &mut Vec<u8>) {
        buf.push(u8::from(*self));
    }

    fn encode_size(&self) -> usize {
        1
    }
}

impl Decode for bool {
    fn decode(data: &[u8]) -> Option<(Self, usize)> {
        if data.is_empty() {
            return None;
        }
        match data[0] {
            0 => Some((false, 1)),
            1 => Some((true, 1)),
            _ => None,
        }
    }
}

// Vec<u8>: 4-byte LE length prefix followed by raw bytes.
impl Encode for Vec<u8> {
    fn encode(&self, buf: &mut Vec<u8>) {
        let len: u32 = self.len().try_into().unwrap_or_else(|_| {
            panic!(
                "conduit: payload too large ({} bytes exceeds u32::MAX)",
                self.len()
            )
        });
        buf.extend_from_slice(&len.to_le_bytes());
        buf.extend_from_slice(self);
    }

    fn encode_size(&self) -> usize {
        4 + self.len()
    }
}

impl Decode for Vec<u8> {
    fn decode(data: &[u8]) -> Option<(Self, usize)> {
        if data.len() < 4 {
            return None;
        }
        let len = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
        let total = 4 + len;
        if data.len() < total {
            return None;
        }
        Some((data[4..total].to_vec(), total))
    }
}

// String: 4-byte LE length prefix followed by UTF-8 bytes.
impl Encode for String {
    fn encode(&self, buf: &mut Vec<u8>) {
        let len: u32 = self.len().try_into().unwrap_or_else(|_| {
            panic!(
                "conduit: payload too large ({} bytes exceeds u32::MAX)",
                self.len()
            )
        });
        buf.extend_from_slice(&len.to_le_bytes());
        buf.extend_from_slice(self.as_bytes());
    }

    fn encode_size(&self) -> usize {
        4 + self.len()
    }
}

impl Decode for String {
    fn decode(data: &[u8]) -> Option<(Self, usize)> {
        if data.len() < 4 {
            return None;
        }
        let len = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
        let total = 4 + len;
        if data.len() < total {
            return None;
        }
        let s = std::str::from_utf8(&data[4..total]).ok()?;
        Some((s.to_owned(), total))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_header_roundtrip() {
        let original = FrameHeader {
            version: PROTOCOL_VERSION,
            reserved: 0,
            msg_type: MsgType::Request,
            sequence: 42,
            payload_len: 128,
        };
        let mut buf = Vec::new();
        original.write_to(&mut buf);
        assert_eq!(buf.len(), FRAME_HEADER_SIZE);
        let parsed = FrameHeader::read_from(&buf).unwrap();
        assert_eq!(original, parsed);
    }

    #[test]
    fn frame_pack_unwrap() {
        let header = FrameHeader {
            version: PROTOCOL_VERSION,
            reserved: 0,
            msg_type: MsgType::Push,
            sequence: 7,
            payload_len: 5,
        };
        let payload = b"hello";
        let frame = frame_pack(&header, payload);
        assert_eq!(frame.len(), FRAME_HEADER_SIZE + 5);

        let (parsed_header, parsed_payload) = frame_unpack(&frame).unwrap();
        assert_eq!(parsed_header, header);
        assert_eq!(parsed_payload, payload);
    }

    #[test]
    fn frame_too_short() {
        let short = [0u8; 5];
        assert!(FrameHeader::read_from(&short).is_none());
        assert!(frame_unpack(&short).is_none());
    }

    #[test]
    fn encode_decode_primitives() {
        // u8
        let mut buf = Vec::new();
        42u8.encode(&mut buf);
        let (val, consumed) = u8::decode(&buf).unwrap();
        assert_eq!(val, 42u8);
        assert_eq!(consumed, 1);

        // u32
        buf.clear();
        0xDEAD_BEEFu32.encode(&mut buf);
        let (val, consumed) = u32::decode(&buf).unwrap();
        assert_eq!(val, 0xDEAD_BEEFu32);
        assert_eq!(consumed, 4);

        // i64
        buf.clear();
        (-999_999i64).encode(&mut buf);
        let (val, consumed) = i64::decode(&buf).unwrap();
        assert_eq!(val, -999_999i64);
        assert_eq!(consumed, 8);

        // f64
        buf.clear();
        std::f64::consts::PI.encode(&mut buf);
        let (val, consumed) = f64::decode(&buf).unwrap();
        assert_eq!(val, std::f64::consts::PI);
        assert_eq!(consumed, 8);

        // bool
        buf.clear();
        true.encode(&mut buf);
        let (val, consumed) = bool::decode(&buf).unwrap();
        assert!(val);
        assert_eq!(consumed, 1);

        buf.clear();
        false.encode(&mut buf);
        let (val, consumed) = bool::decode(&buf).unwrap();
        assert!(!val);
        assert_eq!(consumed, 1);
    }

    #[test]
    fn encode_decode_vec() {
        let original: Vec<u8> = vec![0xCA, 0xFE, 0xBA, 0xBE];
        let mut buf = Vec::new();
        original.encode(&mut buf);
        assert_eq!(buf.len(), 4 + 4); // 4-byte length + 4 bytes
        let (decoded, consumed) = Vec::<u8>::decode(&buf).unwrap();
        assert_eq!(decoded, original);
        assert_eq!(consumed, 8);
    }

    #[test]
    fn encode_decode_string() {
        let original = String::from("conduit transport layer");
        let mut buf = Vec::new();
        original.encode(&mut buf);
        assert_eq!(buf.len(), 4 + original.len());
        let (decoded, consumed) = String::decode(&buf).unwrap();
        assert_eq!(decoded, original);
        assert_eq!(consumed, 4 + original.len());
    }
}
