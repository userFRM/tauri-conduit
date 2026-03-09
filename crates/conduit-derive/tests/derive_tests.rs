use conduit_core::{Decode, Encode, Router};
use conduit_derive::{Decode, Encode, conduit_command};

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

// ---------------------------------------------------------------------------
// 9. #[conduit_command] attribute macro
// ---------------------------------------------------------------------------

#[conduit_command]
fn greet(name: String, greeting: String) -> String {
    format!("{greeting}, {name}!")
}

#[test]
fn conduit_command_named_params() {
    let router = Router::new();
    router.register_json("greet", greet);

    // Frontend sends { "name": "Alice", "greeting": "Hello" }
    let payload = serde_json::to_vec(&serde_json::json!({
        "name": "Alice",
        "greeting": "Hello"
    }))
    .unwrap();
    let resp = router.call("greet", payload).unwrap();
    let result: String = serde_json::from_slice(&resp).unwrap();
    assert_eq!(result, "Hello, Alice!");
}

#[conduit_command]
fn divide(a: f64, b: f64) -> Result<f64, String> {
    if b == 0.0 {
        Err("division by zero".into())
    } else {
        Ok(a / b)
    }
}

#[test]
fn conduit_command_result_ok() {
    let router = Router::new();
    router.register_json_result("divide", divide);

    let payload = serde_json::to_vec(&serde_json::json!({ "a": 10.0, "b": 2.0 })).unwrap();
    let resp = router.call("divide", payload).unwrap();
    let result: f64 = serde_json::from_slice(&resp).unwrap();
    assert!((result - 5.0).abs() < f64::EPSILON);
}

#[test]
fn conduit_command_result_err() {
    let router = Router::new();
    router.register_json_result("divide", divide);

    let payload = serde_json::to_vec(&serde_json::json!({ "a": 10.0, "b": 0.0 })).unwrap();
    let err = router.call("divide", payload).unwrap_err();
    assert_eq!(err.to_string(), "handler error: division by zero");
}

#[conduit_command]
fn ping() -> String {
    "pong".to_string()
}

#[test]
fn conduit_command_zero_params() {
    let router = Router::new();
    router.register_json("ping", ping);

    let payload = serde_json::to_vec(&()).unwrap(); // null
    let resp = router.call("ping", payload).unwrap();
    let result: String = serde_json::from_slice(&resp).unwrap();
    assert_eq!(result, "pong");
}

#[conduit_command]
fn echo_name(name: String) -> String {
    name
}

#[test]
fn conduit_command_single_param() {
    let router = Router::new();
    router.register_json("echo_name", echo_name);

    let payload = serde_json::to_vec(&serde_json::json!({ "name": "test" })).unwrap();
    let resp = router.call("echo_name", payload).unwrap();
    let result: String = serde_json::from_slice(&resp).unwrap();
    assert_eq!(result, "test");
}
