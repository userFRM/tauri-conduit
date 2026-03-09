use conduit_core::{Decode, Encode};
use conduit_derive::{Decode, Encode};

// ---------------------------------------------------------------------------
// Test structs
// ---------------------------------------------------------------------------

#[derive(Debug, PartialEq, Encode, Decode)]
struct SimplePrimitives {
    a: u8,
    b: u32,
    c: i64,
    d: f64,
    e: bool,
}

#[derive(Debug, PartialEq, Encode, Decode)]
struct VarLength {
    payload: Vec<u8>,
    label: String,
}

#[derive(Debug, PartialEq, Encode, Decode)]
struct Empty {}

#[derive(Debug, PartialEq, Encode, Decode)]
struct SingleField {
    value: u32,
}

#[derive(Debug, PartialEq, Encode, Decode)]
struct Alpha {
    x: u16,
    y: u16,
}

/// Regression test: field named `data` must not shadow the decode parameter.
#[derive(Debug, PartialEq, Encode, Decode)]
struct HasDataField {
    data: Vec<u8>,
    tag: u32,
}

#[derive(Debug, PartialEq, Encode, Decode)]
struct Beta {
    flag: bool,
    name: String,
}

// ---------------------------------------------------------------------------
// 1. Simple struct roundtrip
// ---------------------------------------------------------------------------

#[test]
fn simple_struct_roundtrip() {
    let original = SimplePrimitives {
        a: 0xFF,
        b: 0xDEAD_BEEF,
        c: -123_456_789_012,
        d: std::f64::consts::E,
        e: true,
    };

    let mut buf = Vec::new();
    original.encode(&mut buf);

    let (decoded, consumed) = SimplePrimitives::decode(&buf).unwrap();
    assert_eq!(decoded, original);
    assert_eq!(consumed, buf.len());
}

// ---------------------------------------------------------------------------
// 2. Struct with Vec<u8> and String
// ---------------------------------------------------------------------------

#[test]
fn variable_length_fields_roundtrip() {
    let original = VarLength {
        payload: vec![0xCA, 0xFE, 0xBA, 0xBE, 0x00, 0x01],
        label: String::from("conduit-derive integration test"),
    };

    let mut buf = Vec::new();
    original.encode(&mut buf);

    let (decoded, consumed) = VarLength::decode(&buf).unwrap();
    assert_eq!(decoded, original);
    assert_eq!(consumed, buf.len());
}

#[test]
fn variable_length_empty_contents() {
    let original = VarLength {
        payload: vec![],
        label: String::new(),
    };

    let mut buf = Vec::new();
    original.encode(&mut buf);

    // Two 4-byte length prefixes, both zero.
    assert_eq!(buf.len(), 4 + 4);

    let (decoded, consumed) = VarLength::decode(&buf).unwrap();
    assert_eq!(decoded, original);
    assert_eq!(consumed, buf.len());
}

// ---------------------------------------------------------------------------
// 3. Empty struct
// ---------------------------------------------------------------------------

#[test]
fn empty_struct_roundtrip() {
    let original = Empty {};

    let mut buf = Vec::new();
    original.encode(&mut buf);

    assert!(buf.is_empty(), "empty struct should produce zero bytes");
    assert_eq!(original.encode_size(), 0);

    let (decoded, consumed) = Empty::decode(&buf).unwrap();
    assert_eq!(decoded, original);
    assert_eq!(consumed, 0);
}

// ---------------------------------------------------------------------------
// 4. Single-field struct
// ---------------------------------------------------------------------------

#[test]
fn single_field_roundtrip() {
    let original = SingleField { value: 42 };

    let mut buf = Vec::new();
    original.encode(&mut buf);

    assert_eq!(buf.len(), 4);

    let (decoded, consumed) = SingleField::decode(&buf).unwrap();
    assert_eq!(decoded, original);
    assert_eq!(consumed, 4);
}

// ---------------------------------------------------------------------------
// 5. encode_size accuracy
// ---------------------------------------------------------------------------

#[test]
fn encode_size_matches_encoded_len_primitives() {
    let s = SimplePrimitives {
        a: 1,
        b: 2,
        c: 3,
        d: 4.0,
        e: false,
    };
    let mut buf = Vec::new();
    s.encode(&mut buf);
    assert_eq!(
        s.encode_size(),
        buf.len(),
        "encode_size() must equal actual encoded length for SimplePrimitives"
    );
    // Expected: u8(1) + u32(4) + i64(8) + f64(8) + bool(1) = 22
    assert_eq!(s.encode_size(), 22);
}

#[test]
fn encode_size_matches_encoded_len_variable() {
    let v = VarLength {
        payload: vec![1, 2, 3],
        label: String::from("hello"),
    };
    let mut buf = Vec::new();
    v.encode(&mut buf);
    assert_eq!(
        v.encode_size(),
        buf.len(),
        "encode_size() must equal actual encoded length for VarLength"
    );
    // Expected: (4 + 3) + (4 + 5) = 16
    assert_eq!(v.encode_size(), 16);
}

#[test]
fn encode_size_matches_encoded_len_empty() {
    let e = Empty {};
    let mut buf = Vec::new();
    e.encode(&mut buf);
    assert_eq!(e.encode_size(), buf.len());
    assert_eq!(e.encode_size(), 0);
}

#[test]
fn encode_size_matches_encoded_len_single() {
    let s = SingleField { value: 99 };
    let mut buf = Vec::new();
    s.encode(&mut buf);
    assert_eq!(s.encode_size(), buf.len());
    assert_eq!(s.encode_size(), 4);
}

// ---------------------------------------------------------------------------
// 6. Partial decode failure (truncated buffer)
// ---------------------------------------------------------------------------

#[test]
fn truncated_buffer_returns_none() {
    let original = SimplePrimitives {
        a: 1,
        b: 2,
        c: 3,
        d: 4.0,
        e: true,
    };
    let mut buf = Vec::new();
    original.encode(&mut buf);

    // Try every possible truncation length (except the full buffer).
    for cut in 0..buf.len() {
        assert!(
            SimplePrimitives::decode(&buf[..cut]).is_none(),
            "should fail to decode with only {cut}/{} bytes",
            buf.len()
        );
    }
}

#[test]
fn truncated_variable_length_returns_none() {
    let original = VarLength {
        payload: vec![0xAA, 0xBB],
        label: String::from("x"),
    };
    let mut buf = Vec::new();
    original.encode(&mut buf);

    for cut in 0..buf.len() {
        assert!(
            VarLength::decode(&buf[..cut]).is_none(),
            "should fail to decode VarLength with only {cut}/{} bytes",
            buf.len()
        );
    }
}

// ---------------------------------------------------------------------------
// 7. Multiple structs in same payload (back-to-back)
// ---------------------------------------------------------------------------

#[test]
fn multiple_structs_back_to_back() {
    let alpha = Alpha {
        x: 0x1234,
        y: 0x5678,
    };
    let beta = Beta {
        flag: true,
        name: String::from("conduit"),
    };

    // Encode both into a single buffer.
    let mut buf = Vec::new();
    alpha.encode(&mut buf);
    beta.encode(&mut buf);

    // Decode Alpha from the start.
    let (decoded_alpha, alpha_len) = Alpha::decode(&buf).unwrap();
    assert_eq!(decoded_alpha, alpha);

    // Decode Beta from the remaining bytes.
    let (decoded_beta, beta_len) = Beta::decode(&buf[alpha_len..]).unwrap();
    assert_eq!(decoded_beta, beta);

    // Consumed offsets should cover the entire buffer.
    assert_eq!(alpha_len + beta_len, buf.len());
}

#[test]
fn multiple_structs_encode_size_sum() {
    let alpha = Alpha { x: 100, y: 200 };
    let beta = Beta {
        flag: false,
        name: String::from("test"),
    };

    let mut buf = Vec::new();
    alpha.encode(&mut buf);
    beta.encode(&mut buf);

    assert_eq!(
        alpha.encode_size() + beta.encode_size(),
        buf.len(),
        "sum of encode_size() must equal combined encoded length"
    );
}

// ---------------------------------------------------------------------------
// 8. Regression: field named `data` must not shadow decode parameter
// ---------------------------------------------------------------------------

#[test]
fn field_named_data_does_not_shadow() {
    let original = HasDataField {
        data: vec![0xDE, 0xAD],
        tag: 42,
    };
    let mut buf = Vec::new();
    original.encode(&mut buf);

    let (decoded, consumed) = HasDataField::decode(&buf).unwrap();
    assert_eq!(decoded, original);
    assert_eq!(consumed, buf.len());
    // Verify the tag field (after data) decoded correctly — this was the bug.
    assert_eq!(decoded.tag, 42);
}
