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

/// Per-frame overhead in the drain wire format: 4 bytes for the u32 LE length prefix.
pub const DRAIN_FRAME_OVERHEAD: usize = 4;

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
    ///
    /// **Warning:** `Other(v)` where `v` matches a known variant (0x00, 0x01,
    /// 0x02, 0x04) will NOT roundtrip: `MsgType::from_u8(v)` returns the
    /// named variant, not `Other(v)`. Use values `>= 0x10` for custom types
    /// to avoid aliasing.
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
        let seq = self.sequence.to_le_bytes();
        let plen = self.payload_len.to_le_bytes();
        let header: [u8; FRAME_HEADER_SIZE] = [
            self.version,
            self.reserved,
            self.msg_type.to_u8(),
            seq[0],
            seq[1],
            seq[2],
            seq[3],
            plen[0],
            plen[1],
            plen[2],
            plen[3],
        ];
        buf.extend_from_slice(&header);
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
        if version != PROTOCOL_VERSION {
            return None;
        }
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
    assert_eq!(
        header.payload_len as usize,
        payload.len(),
        "frame_pack: header.payload_len ({}) != payload.len() ({})",
        header.payload_len,
        payload.len(),
    );
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
    let payload_end = FRAME_HEADER_SIZE.checked_add(header.payload_len as usize)?;
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
    /// Minimum number of bytes required to attempt decoding this type.
    ///
    /// For fixed-size types (primitives), this equals the exact encoded size.
    /// For variable-size types (String, Vec), this is the minimum (the length
    /// prefix size). Used by derived impls for an upfront bounds check.
    const MIN_SIZE: usize = 0;

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
                const MIN_SIZE: usize = std::mem::size_of::<$ty>();

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
    const MIN_SIZE: usize = 1;

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

// Vec<T>: 4-byte LE element count followed by each element encoded in sequence.
// For Vec<u8>, this produces the same wire format as a length-prefixed byte blob
// (count + N individual bytes = count + N raw bytes).
impl<T: Encode> Encode for Vec<T> {
    fn encode(&self, buf: &mut Vec<u8>) {
        let count: u32 = self.len().try_into().unwrap_or_else(|_| {
            panic!(
                "conduit: vec too large ({} elements exceeds u32::MAX)",
                self.len()
            )
        });
        buf.extend_from_slice(&count.to_le_bytes());
        for item in self {
            item.encode(buf);
        }
    }

    fn encode_size(&self) -> usize {
        4 + self.iter().map(|item| item.encode_size()).sum::<usize>()
    }
}

impl<T: Decode> Decode for Vec<T> {
    const MIN_SIZE: usize = 4;

    fn decode(data: &[u8]) -> Option<(Self, usize)> {
        if data.len() < 4 {
            return None;
        }
        let count = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
        let mut off = 4;
        let mut items = Vec::with_capacity(count);
        for _ in 0..count {
            let (item, consumed) = T::decode(&data[off..])?;
            off += consumed;
            items.push(item);
        }
        Some((items, off))
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
    const MIN_SIZE: usize = 4;

    fn decode(data: &[u8]) -> Option<(Self, usize)> {
        if data.len() < 4 {
            return None;
        }
        let len = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
        // Length cannot exceed remaining buffer
        if len > data.len() - 4 {
            return None;
        }
        let total = 4 + len;
        let s = std::str::from_utf8(&data[4..total]).ok()?;
        Some((s.to_owned(), total))
    }
}

// ---------------------------------------------------------------------------
// Bytes: optimized Vec<u8> wrapper with bulk encode/decode
// ---------------------------------------------------------------------------

/// A newtype wrapper around `Vec<u8>` with optimized binary Encode/Decode.
///
/// Unlike `Vec<u8>` which goes through the generic `Vec<T>` impl (decoding
/// each byte individually), `Bytes` uses a single bulk copy for both encoding
/// and decoding.
///
/// The wire format is identical to `Vec<u8>`: `[u32 LE count][bytes...]`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct Bytes(pub Vec<u8>);

impl From<Vec<u8>> for Bytes {
    fn from(v: Vec<u8>) -> Self {
        Self(v)
    }
}

impl From<Bytes> for Vec<u8> {
    fn from(b: Bytes) -> Self {
        b.0
    }
}

impl std::ops::Deref for Bytes {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        &self.0
    }
}

impl AsRef<[u8]> for Bytes {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl Encode for Bytes {
    fn encode(&self, buf: &mut Vec<u8>) {
        let count: u32 = self.0.len().try_into().unwrap_or_else(|_| {
            panic!(
                "conduit: bytes too large ({} bytes exceeds u32::MAX)",
                self.0.len()
            )
        });
        buf.extend_from_slice(&count.to_le_bytes());
        buf.extend_from_slice(&self.0);
    }

    fn encode_size(&self) -> usize {
        4 + self.0.len()
    }
}

impl Decode for Bytes {
    const MIN_SIZE: usize = 4;

    fn decode(data: &[u8]) -> Option<(Self, usize)> {
        if data.len() < 4 {
            return None;
        }
        let count = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
        let total = 4usize.checked_add(count)?;
        if data.len() < total {
            return None;
        }
        Some((Bytes(data[4..total].to_vec()), total))
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

    #[test]
    fn encode_decode_bytes() {
        let original = Bytes(vec![10, 20, 30, 40, 50]);
        let mut buf = Vec::new();
        original.encode(&mut buf);
        assert_eq!(original.encode_size(), buf.len());
        let (decoded, consumed) = Bytes::decode(&buf).unwrap();
        assert_eq!(decoded, original);
        assert_eq!(consumed, buf.len());
    }

    #[test]
    fn bytes_empty() {
        let original = Bytes(Vec::new());
        let mut buf = Vec::new();
        original.encode(&mut buf);
        assert_eq!(buf.len(), 4); // just the count
        let (decoded, consumed) = Bytes::decode(&buf).unwrap();
        assert_eq!(decoded.0.len(), 0);
        assert_eq!(consumed, 4);
    }

    #[test]
    fn bytes_wire_compatible_with_vec_u8() {
        // Verify Bytes and Vec<u8> produce identical wire format
        let data: Vec<u8> = vec![1, 2, 3, 4, 5];
        let bytes = Bytes(data.clone());

        let mut buf_vec = Vec::new();
        data.encode(&mut buf_vec);

        let mut buf_bytes = Vec::new();
        bytes.encode(&mut buf_bytes);

        assert_eq!(
            buf_vec, buf_bytes,
            "Bytes and Vec<u8> must produce identical wire format"
        );
    }

    #[test]
    fn min_size_primitives() {
        assert_eq!(<u8 as Decode>::MIN_SIZE, 1);
        assert_eq!(<u16 as Decode>::MIN_SIZE, 2);
        assert_eq!(<u32 as Decode>::MIN_SIZE, 4);
        assert_eq!(<u64 as Decode>::MIN_SIZE, 8);
        assert_eq!(<i8 as Decode>::MIN_SIZE, 1);
        assert_eq!(<i16 as Decode>::MIN_SIZE, 2);
        assert_eq!(<i32 as Decode>::MIN_SIZE, 4);
        assert_eq!(<i64 as Decode>::MIN_SIZE, 8);
        assert_eq!(<f32 as Decode>::MIN_SIZE, 4);
        assert_eq!(<f64 as Decode>::MIN_SIZE, 8);
        assert_eq!(<bool as Decode>::MIN_SIZE, 1);
        assert_eq!(<String as Decode>::MIN_SIZE, 4);
        assert_eq!(<Vec<u8> as Decode>::MIN_SIZE, 4);
        assert_eq!(<Bytes as Decode>::MIN_SIZE, 4);
    }
}
