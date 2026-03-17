use std::sync::Arc;

use conduit_core::{ConduitHandler, Decode, Encode, HandlerResponse};
use conduit_derive::{Decode, Encode, command, handler};

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
// Helper: unwrap a sync HandlerResponse
// ---------------------------------------------------------------------------

fn call_sync(
    handler: &dyn ConduitHandler,
    payload: Vec<u8>,
) -> Result<Vec<u8>, conduit_core::Error> {
    let ctx: Arc<dyn std::any::Any + Send + Sync> = Arc::new(());
    match handler.call(payload, ctx) {
        HandlerResponse::Sync(result) => result,
        HandlerResponse::Async(_) => panic!("expected HandlerResponse::Sync"),
    }
}

// ---------------------------------------------------------------------------
// 9. #[command] attribute macro — sync handlers
// ---------------------------------------------------------------------------

#[command]
fn greet_v2(name: String, greeting: String) -> String {
    format!("{greeting}, {name}!")
}

#[test]
fn command_sync_named_params() {
    let payload = serde_json::to_vec(&serde_json::json!({
        "name": "Alice",
        "greeting": "Hello"
    }))
    .unwrap();
    let resp = call_sync(&handler!(greet_v2), payload).unwrap();
    let result: String = sonic_rs::from_slice(&resp).unwrap();
    assert_eq!(result, "Hello, Alice!");
}

#[command]
fn divide_v2(a: f64, b: f64) -> Result<f64, String> {
    if b == 0.0 {
        Err("division by zero".into())
    } else {
        Ok(a / b)
    }
}

#[test]
fn command_sync_result_ok() {
    let payload = serde_json::to_vec(&serde_json::json!({ "a": 10.0, "b": 2.0 })).unwrap();
    let resp = call_sync(&handler!(divide_v2), payload).unwrap();
    let result: f64 = sonic_rs::from_slice(&resp).unwrap();
    assert!((result - 5.0).abs() < f64::EPSILON);
}

#[test]
fn command_sync_result_err() {
    let payload = serde_json::to_vec(&serde_json::json!({ "a": 10.0, "b": 0.0 })).unwrap();
    let err = call_sync(&handler!(divide_v2), payload).unwrap_err();
    assert_eq!(err.to_string(), "handler error: division by zero");
}

#[command]
fn ping_v2() -> String {
    "pong".to_string()
}

#[test]
fn command_sync_zero_params() {
    let resp = call_sync(&handler!(ping_v2), vec![]).unwrap();
    let result: String = sonic_rs::from_slice(&resp).unwrap();
    assert_eq!(result, "pong");
}

#[command]
fn echo_name_v2(name: String) -> String {
    name
}

#[test]
fn command_sync_single_param() {
    let payload = serde_json::to_vec(&serde_json::json!({ "name": "test" })).unwrap();
    let resp = call_sync(&handler!(echo_name_v2), payload).unwrap();
    let result: String = sonic_rs::from_slice(&resp).unwrap();
    assert_eq!(result, "test");
}

#[command]
fn add_v2(a: i32, b: i32) -> i32 {
    a + b
}

#[test]
fn command_sync_non_result_return() {
    let payload = serde_json::to_vec(&serde_json::json!({ "a": 3, "b": 4 })).unwrap();
    let resp = call_sync(&handler!(add_v2), payload).unwrap();
    let result: i32 = sonic_rs::from_slice(&resp).unwrap();
    assert_eq!(result, 7);
}

// ---------------------------------------------------------------------------
// 10. #[command] attribute macro — async handlers
// ---------------------------------------------------------------------------

#[command]
async fn async_greet(name: String) -> String {
    format!("Hello, {name}!")
}

#[test]
fn command_async_basic() {
    let payload = serde_json::to_vec(&serde_json::json!({ "name": "World" })).unwrap();
    let ctx: Arc<dyn std::any::Any + Send + Sync> = Arc::new(());

    match handler!(async_greet).call(payload, ctx) {
        HandlerResponse::Async(future) => {
            let rt = tokio::runtime::Runtime::new().unwrap();
            let result = rt.block_on(future).unwrap();
            let value: String = sonic_rs::from_slice(&result).unwrap();
            assert_eq!(value, "Hello, World!");
        }
        HandlerResponse::Sync(_) => panic!("expected HandlerResponse::Async"),
    }
}

#[command]
async fn async_divide(a: f64, b: f64) -> Result<f64, String> {
    if b == 0.0 {
        Err(String::from("division by zero"))
    } else {
        Ok(a / b)
    }
}

#[test]
fn command_async_result_ok() {
    let payload = serde_json::to_vec(&serde_json::json!({ "a": 10.0, "b": 2.0 })).unwrap();
    let ctx: Arc<dyn std::any::Any + Send + Sync> = Arc::new(());
    let rt = tokio::runtime::Runtime::new().unwrap();

    match handler!(async_divide).call(payload, ctx) {
        HandlerResponse::Async(future) => {
            let resp = rt.block_on(future).unwrap();
            let result: f64 = sonic_rs::from_slice(&resp).unwrap();
            assert!((result - 5.0).abs() < f64::EPSILON);
        }
        _ => panic!("expected async"),
    }
}

#[test]
fn command_async_result_err() {
    let payload = serde_json::to_vec(&serde_json::json!({ "a": 10.0, "b": 0.0 })).unwrap();
    let ctx: Arc<dyn std::any::Any + Send + Sync> = Arc::new(());
    let rt = tokio::runtime::Runtime::new().unwrap();

    match handler!(async_divide).call(payload, ctx) {
        HandlerResponse::Async(future) => {
            let err = rt.block_on(future).unwrap_err();
            assert_eq!(err.to_string(), "handler error: division by zero");
        }
        _ => panic!("expected async"),
    }
}

// ---------------------------------------------------------------------------
// 11. #[command] edge cases — bad input, zero-param async
// ---------------------------------------------------------------------------

#[command]
async fn async_ping() -> String {
    "async pong".to_string()
}

#[test]
fn command_async_zero_params() {
    let ctx: Arc<dyn std::any::Any + Send + Sync> = Arc::new(());
    let rt = tokio::runtime::Runtime::new().unwrap();

    match handler!(async_ping).call(vec![], ctx) {
        HandlerResponse::Async(future) => {
            let resp = rt.block_on(future).unwrap();
            let value: String = sonic_rs::from_slice(&resp).unwrap();
            assert_eq!(value, "async pong");
        }
        _ => panic!("expected async"),
    }
}

#[test]
fn command_sync_bad_json() {
    let err = call_sync(&handler!(greet_v2), b"not json".to_vec()).unwrap_err();
    assert!(matches!(err, conduit_core::Error::Serialize(_)));
}

#[test]
fn command_async_bad_json() {
    let ctx: Arc<dyn std::any::Any + Send + Sync> = Arc::new(());
    let rt = tokio::runtime::Runtime::new().unwrap();

    match handler!(greet_v2).call(b"not json".to_vec(), Arc::new(())) {
        HandlerResponse::Sync(Err(e)) => {
            assert!(matches!(e, conduit_core::Error::Serialize(_)));
        }
        _other => panic!("expected Sync(Err(Serialize)), got something else"),
    }

    // Also test the async path
    match handler!(async_greet).call(b"not json".to_vec(), ctx) {
        HandlerResponse::Async(future) => {
            let err = rt.block_on(future).unwrap_err();
            assert!(matches!(err, conduit_core::Error::Serialize(_)));
        }
        _ => panic!("expected async"),
    }
}

#[test]
fn command_sync_empty_payload_with_params() {
    let err = call_sync(&handler!(greet_v2), vec![]).unwrap_err();
    assert!(matches!(err, conduit_core::Error::Serialize(_)));
}

// ---------------------------------------------------------------------------
// 12. Unit return type
// ---------------------------------------------------------------------------

#[command]
fn fire_and_forget(message: String) {
    let _ = message; // side effect only
}

#[test]
fn command_sync_unit_return() {
    let payload = serde_json::to_vec(&serde_json::json!({ "message": "hello" })).unwrap();
    let resp = call_sync(&handler!(fire_and_forget), payload).unwrap();
    // Unit `()` serializes to `null` in JSON.
    let result: serde_json::Value = serde_json::from_slice(&resp).unwrap();
    assert_eq!(result, serde_json::Value::Null);
}

#[command]
async fn async_fire_and_forget(message: String) {
    let _ = message;
}

#[test]
fn command_async_unit_return() {
    let payload = serde_json::to_vec(&serde_json::json!({ "message": "hello" })).unwrap();
    let ctx: Arc<dyn std::any::Any + Send + Sync> = Arc::new(());
    let rt = tokio::runtime::Runtime::new().unwrap();

    match handler!(async_fire_and_forget).call(payload, ctx) {
        HandlerResponse::Async(future) => {
            let resp = rt.block_on(future).unwrap();
            let result: serde_json::Value = serde_json::from_slice(&resp).unwrap();
            assert_eq!(result, serde_json::Value::Null);
        }
        _ => panic!("expected async"),
    }
}

// ---------------------------------------------------------------------------
// 13. Multi-word parameter names use camelCase in JSON (matching Tauri)
// ---------------------------------------------------------------------------

#[command]
fn greet_with_full_name(first_name: String, last_name: String) -> String {
    format!("{first_name} {last_name}")
}

#[test]
fn command_camel_case_params() {
    // Rust snake_case params → camelCase JSON keys (matching Tauri's behavior).
    let payload = serde_json::to_vec(&serde_json::json!({
        "firstName": "Alice",
        "lastName": "Smith"
    }))
    .unwrap();
    let resp = call_sync(&handler!(greet_with_full_name), payload).unwrap();
    let result: String = sonic_rs::from_slice(&resp).unwrap();
    assert_eq!(result, "Alice Smith");
}

// ---------------------------------------------------------------------------
// 14. Decode shadowing regression — field named __conduit_n
// ---------------------------------------------------------------------------

#[derive(Debug, PartialEq, Encode, Decode)]
struct HasConduitFields {
    __conduit_n: u32,
    other: u32,
}

#[test]
fn field_named_conduit_n_does_not_shadow() {
    let original = HasConduitFields {
        __conduit_n: 42,
        other: 99,
    };
    let mut buf = Vec::new();
    original.encode(&mut buf);
    let (decoded, consumed) = HasConduitFields::decode(&buf).unwrap();
    assert_eq!(decoded, original);
    assert_eq!(consumed, buf.len());
}

// ---------------------------------------------------------------------------
// C4 — ShadowTest: fields named after old internal variables
// ---------------------------------------------------------------------------

#[derive(Debug, PartialEq, Encode, Decode)]
struct ShadowTest {
    __conduit_buf: u32,
    __conduit_pos: u32,
    __conduit_val: u32,
    __conduit_n: u32,
}

#[test]
fn shadow_test_roundtrip() {
    let original = ShadowTest {
        __conduit_buf: 1,
        __conduit_pos: 2,
        __conduit_val: 3,
        __conduit_n: 4,
    };
    let mut buf = Vec::new();
    original.encode(&mut buf);
    let (decoded, consumed) = ShadowTest::decode(&buf).unwrap();
    assert_eq!(decoded, original);
    assert_eq!(consumed, buf.len());
}

// ---------------------------------------------------------------------------
// H7. Option<T> parameter support via #[serde(default)]
// ---------------------------------------------------------------------------

#[command]
fn greet_optional(name: String, title: Option<u32>) -> String {
    match title {
        Some(t) => format!("{name} (title={t})"),
        None => name,
    }
}

#[test]
fn command_option_param_present() {
    let payload = serde_json::to_vec(&serde_json::json!({ "name": "Alice", "title": 42 })).unwrap();
    let resp = call_sync(&handler!(greet_optional), payload).unwrap();
    let result: String = sonic_rs::from_slice(&resp).unwrap();
    assert_eq!(result, "Alice (title=42)");
}

#[test]
fn command_option_param_missing() {
    // When the field is absent from JSON, #[serde(default)] should fill None.
    let payload = serde_json::to_vec(&serde_json::json!({ "name": "Bob" })).unwrap();
    let resp = call_sync(&handler!(greet_optional), payload).unwrap();
    let result: String = sonic_rs::from_slice(&resp).unwrap();
    assert_eq!(result, "Bob");
}

// ---------------------------------------------------------------------------
// 15. Multi-word params use camelCase in JSON
// ---------------------------------------------------------------------------

#[command]
fn three_word_params(my_long_name: String, http_status_code: u16) -> String {
    format!("{my_long_name}: {http_status_code}")
}

#[test]
fn command_three_word_camel_case() {
    let payload = serde_json::to_vec(&serde_json::json!({
        "myLongName": "test",
        "httpStatusCode": 200
    }))
    .unwrap();
    let resp = call_sync(&handler!(three_word_params), payload).unwrap();
    let result: String = sonic_rs::from_slice(&resp).unwrap();
    assert_eq!(result, "test: 200");
}

// ---------------------------------------------------------------------------
// 16. Preserved original functions — callable directly
// ---------------------------------------------------------------------------

#[test]
fn original_sync_function_preserved() {
    // #[command] preserves the original function — call it directly.
    assert_eq!(greet_v2("Alice".into(), "Hello".into()), "Hello, Alice!");
    assert_eq!(add_v2(3, 4), 7);
    assert_eq!(ping_v2(), "pong");
    assert_eq!(echo_name_v2("test".into()), "test");
}

#[test]
fn original_sync_result_function_preserved() {
    assert!((divide_v2(10.0, 2.0).unwrap() - 5.0).abs() < f64::EPSILON);
    assert_eq!(divide_v2(10.0, 0.0).unwrap_err(), "division by zero");
}

#[tokio::test]
async fn original_async_function_preserved() {
    assert_eq!(async_greet("World".into()).await, "Hello, World!");
    assert_eq!(async_ping().await, "async pong");
}

#[tokio::test]
async fn original_async_result_function_preserved() {
    assert!((async_divide(10.0, 2.0).await.unwrap() - 5.0).abs() < f64::EPSILON);
    assert_eq!(
        async_divide(10.0, 0.0).await.unwrap_err(),
        "division by zero"
    );
}

// ---------------------------------------------------------------------------
// 17. MIN_SIZE on derived structs
// ---------------------------------------------------------------------------

#[test]
fn min_size_derived_structs() {
    // SimplePrimitives: u8(1) + u32(4) + i64(8) + f64(8) + bool(1) = 22
    assert_eq!(<SimplePrimitives as Decode>::MIN_SIZE, 22);

    // VarLength: Vec<u8> prefix(4) + String prefix(4) = 8
    assert_eq!(<VarLength as Decode>::MIN_SIZE, 8);

    // Empty: 0
    assert_eq!(<Empty as Decode>::MIN_SIZE, 0);

    // SingleField: u32(4) = 4
    assert_eq!(<SingleField as Decode>::MIN_SIZE, 4);

    // Alpha: u16(2) + u16(2) = 4
    assert_eq!(<Alpha as Decode>::MIN_SIZE, 4);

    // Beta: bool(1) + String prefix(4) = 5
    assert_eq!(<Beta as Decode>::MIN_SIZE, 5);
}
