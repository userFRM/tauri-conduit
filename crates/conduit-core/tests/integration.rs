//! Integration tests for conduit-core.
//!
//! These tests exercise the public API across module boundaries: frames,
//! wire encoding, dispatch, ring buffer, and error paths working together.

use std::sync::Arc;
use std::thread;

use conduit_core::{
    Decode, Encode, Error, FRAME_HEADER_SIZE, FrameHeader, MsgType, PROTOCOL_VERSION, RingBuffer,
    Router, frame_pack, frame_unpack,
};

// ---------------------------------------------------------------------------
// 1. Full roundtrip: build a frame, wrap it, unwrap it, verify all fields
// ---------------------------------------------------------------------------

#[test]
fn full_frame_roundtrip() {
    let payload = b"integration-test-payload";
    let header = FrameHeader {
        version: PROTOCOL_VERSION,
        reserved: 0,
        msg_type: MsgType::Request,
        sequence: 1001,
        payload_len: payload.len() as u32,
    };

    let wire = frame_pack(&header, payload);
    assert_eq!(wire.len(), FRAME_HEADER_SIZE + payload.len());

    let (parsed_header, parsed_payload) = frame_unpack(&wire).unwrap();
    assert_eq!(parsed_header.version, PROTOCOL_VERSION);
    assert_eq!(parsed_header.reserved, 0);
    assert_eq!(parsed_header.msg_type, MsgType::Request);
    assert_eq!(parsed_header.sequence, 1001);
    assert_eq!(parsed_header.payload_len, payload.len() as u32);
    assert_eq!(parsed_payload, payload);
}

#[test]
fn frame_roundtrip_all_msg_types() {
    for msg_type in [
        MsgType::Request,
        MsgType::Response,
        MsgType::Push,
        MsgType::Error,
        MsgType::Other(0x10),
        MsgType::Other(0xFF),
    ] {
        let payload = b"type-check";
        let header = FrameHeader {
            version: PROTOCOL_VERSION,
            reserved: 0,
            msg_type,
            sequence: 0,
            payload_len: payload.len() as u32,
        };
        let wire = frame_pack(&header, payload);
        let (h, p) = frame_unpack(&wire).unwrap();
        assert_eq!(h.msg_type, msg_type);
        assert_eq!(p, payload);
    }
}

#[test]
fn frame_roundtrip_empty_payload() {
    let header = FrameHeader {
        version: PROTOCOL_VERSION,
        reserved: 0,
        msg_type: MsgType::Response,
        sequence: 42,
        payload_len: 0,
    };
    let wire = frame_pack(&header, &[]);
    assert_eq!(wire.len(), FRAME_HEADER_SIZE);

    let (h, p) = frame_unpack(&wire).unwrap();
    assert_eq!(h.payload_len, 0);
    assert!(p.is_empty());
}

// ---------------------------------------------------------------------------
// 2. Encode/Decode with frame
// ---------------------------------------------------------------------------

#[test]
fn encode_struct_fields_in_frame() {
    // Simulate a struct with fields: id (u32), name (String), active (bool)
    let id: u32 = 7;
    let name = String::from("conduit");
    let active: bool = true;

    // Encode all fields into a payload buffer
    let mut payload = Vec::new();
    id.encode(&mut payload);
    name.encode(&mut payload);
    active.encode(&mut payload);

    let expected_size = id.encode_size() + name.encode_size() + active.encode_size();
    assert_eq!(payload.len(), expected_size);

    // Wrap in a frame
    let header = FrameHeader {
        version: PROTOCOL_VERSION,
        reserved: 0,
        msg_type: MsgType::Request,
        sequence: 1,
        payload_len: payload.len() as u32,
    };
    let wire = frame_pack(&header, &payload);

    // Unwrap the frame
    let (_, decoded_payload) = frame_unpack(&wire).unwrap();

    // Decode fields back
    let mut offset = 0;

    let (dec_id, consumed) = u32::decode(&decoded_payload[offset..]).unwrap();
    offset += consumed;
    assert_eq!(dec_id, 7);

    let (dec_name, consumed) = String::decode(&decoded_payload[offset..]).unwrap();
    offset += consumed;
    assert_eq!(dec_name, "conduit");

    let (dec_active, consumed) = bool::decode(&decoded_payload[offset..]).unwrap();
    offset += consumed;
    assert!(dec_active);

    // Should have consumed the entire payload
    assert_eq!(offset, decoded_payload.len());
}

#[test]
fn encode_bytes_in_frame() {
    let data: Vec<u8> = vec![0xDE, 0xAD, 0xBE, 0xEF];
    let mut payload = Vec::new();
    data.encode(&mut payload);

    let header = FrameHeader {
        version: PROTOCOL_VERSION,
        reserved: 0,
        msg_type: MsgType::Push,
        sequence: 99,
        payload_len: payload.len() as u32,
    };
    let wire = frame_pack(&header, &payload);
    let (h, p) = frame_unpack(&wire).unwrap();
    assert_eq!(h.msg_type, MsgType::Push);

    let (decoded, _) = Vec::<u8>::decode(p).unwrap();
    assert_eq!(decoded, vec![0xDE, 0xAD, 0xBE, 0xEF]);
}

// ---------------------------------------------------------------------------
// 3. Router + frame roundtrip
// ---------------------------------------------------------------------------

#[test]
fn dispatch_table_frame_roundtrip() {
    let table = Router::new();

    // Register an "add" command: reads two u32s, returns their sum
    table.register("add", |payload: Vec<u8>| {
        let (a, consumed_a) = u32::decode(&payload).unwrap();
        let (b, _) = u32::decode(&payload[consumed_a..]).unwrap();
        let sum = a + b;
        let mut out = Vec::new();
        sum.encode(&mut out);
        out
    });

    // Build request payload
    let mut req_payload = Vec::new();
    10u32.encode(&mut req_payload);
    32u32.encode(&mut req_payload);

    // Wrap in a request frame
    let req_header = FrameHeader {
        version: PROTOCOL_VERSION,
        reserved: 0,
        msg_type: MsgType::Request,
        sequence: 1,
        payload_len: req_payload.len() as u32,
    };
    let req_wire = frame_pack(&req_header, &req_payload);

    // Server side: unwrap, dispatch, wrap response
    let (req_h, req_p) = frame_unpack(&req_wire).unwrap();
    assert_eq!(req_h.msg_type, MsgType::Request);

    let resp_payload = table.call("add", req_p.to_vec()).unwrap();

    let resp_header = FrameHeader {
        version: PROTOCOL_VERSION,
        reserved: 0,
        msg_type: MsgType::Response,
        sequence: req_h.sequence,
        payload_len: resp_payload.len() as u32,
    };
    let resp_wire = frame_pack(&resp_header, &resp_payload);

    // Client side: unwrap response, decode result
    let (resp_h, resp_p) = frame_unpack(&resp_wire).unwrap();
    assert_eq!(resp_h.msg_type, MsgType::Response);
    assert_eq!(resp_h.sequence, 1);

    let (result, _) = u32::decode(resp_p).unwrap();
    assert_eq!(result, 42);
}

#[test]
fn dispatch_table_unknown_command_response_frame() {
    let table = Router::new();

    let err = table.call("nonexistent", vec![]).unwrap_err();
    assert!(matches!(err, Error::UnknownCommand(ref name) if name == "nonexistent"));

    // Use call_or_error_bytes to get the error as raw bytes for framing
    let resp_payload = table.call_or_error_bytes("nonexistent", vec![]);
    assert_eq!(resp_payload, b"unknown command: nonexistent");

    // Wrap the error in an Error frame
    let header = FrameHeader {
        version: PROTOCOL_VERSION,
        reserved: 0,
        msg_type: MsgType::Error,
        sequence: 0,
        payload_len: resp_payload.len() as u32,
    };
    let wire = frame_pack(&header, &resp_payload);
    let (h, p) = frame_unpack(&wire).unwrap();
    assert_eq!(h.msg_type, MsgType::Error);
    assert_eq!(
        std::str::from_utf8(p).unwrap(),
        "unknown command: nonexistent"
    );
}

#[test]
fn dispatch_register_simple_with_frame() {
    let table = Router::new();
    table.register_simple("ping", || b"pong".to_vec());

    let resp = table.call("ping", vec![]).unwrap();
    let header = FrameHeader {
        version: PROTOCOL_VERSION,
        reserved: 0,
        msg_type: MsgType::Response,
        sequence: 5,
        payload_len: resp.len() as u32,
    };
    let wire = frame_pack(&header, &resp);
    let (_, p) = frame_unpack(&wire).unwrap();
    assert_eq!(p, b"pong");
}

// ---------------------------------------------------------------------------
// 4. RingBuffer + frame
// ---------------------------------------------------------------------------

#[test]
fn ringbuffer_stores_and_drains_framed_data() {
    let rb = RingBuffer::new(4096);

    // Push three framed messages
    for seq in 0u32..3 {
        let payload = format!("msg-{seq}");
        let header = FrameHeader {
            version: PROTOCOL_VERSION,
            reserved: 0,
            msg_type: MsgType::Push,
            sequence: seq,
            payload_len: payload.len() as u32,
        };
        let frame = frame_pack(&header, payload.as_bytes());
        let _ = rb.push(&frame);
    }

    assert_eq!(rb.frame_count(), 3);

    // Drain and verify each frame is intact
    let blob = rb.drain_all();
    assert!(!blob.is_empty());

    // Parse the drain format: [u32 count] [u32 len][bytes]...
    let count = u32::from_le_bytes(blob[0..4].try_into().unwrap()) as usize;
    assert_eq!(count, 3);

    let mut offset = 4;
    for seq in 0u32..3 {
        let len = u32::from_le_bytes(blob[offset..offset + 4].try_into().unwrap()) as usize;
        offset += 4;
        let frame_bytes = &blob[offset..offset + len];
        offset += len;

        // Each drained blob should be a valid conduit frame
        let (h, p) = frame_unpack(frame_bytes).unwrap();
        assert_eq!(h.msg_type, MsgType::Push);
        assert_eq!(h.sequence, seq);
        let expected_payload = format!("msg-{seq}");
        assert_eq!(p, expected_payload.as_bytes());
    }

    assert_eq!(rb.frame_count(), 0);
}

#[test]
fn ringbuffer_pop_yields_intact_frames() {
    let rb = RingBuffer::new(4096);

    let payload = b"pop-test";
    let header = FrameHeader {
        version: PROTOCOL_VERSION,
        reserved: 0,
        msg_type: MsgType::Response,
        sequence: 77,
        payload_len: payload.len() as u32,
    };
    let frame = frame_pack(&header, payload);
    let _ = rb.push(&frame);

    let popped = rb.try_pop().unwrap();
    let (h, p) = frame_unpack(&popped).unwrap();
    assert_eq!(h.sequence, 77);
    assert_eq!(p, b"pop-test");
}

// ---------------------------------------------------------------------------
// 5. Error paths
// ---------------------------------------------------------------------------

#[test]
fn truncated_frame_header() {
    // Less than FRAME_HEADER_SIZE bytes
    let short = vec![PROTOCOL_VERSION, 0, 0x00];
    assert!(frame_unpack(&short).is_none());
}

#[test]
fn truncated_frame_payload() {
    // Valid header claiming 100 bytes of payload, but only 5 provided
    let header = FrameHeader {
        version: PROTOCOL_VERSION,
        reserved: 0,
        msg_type: MsgType::Request,
        sequence: 0,
        payload_len: 100,
    };
    let mut wire = Vec::new();
    header.write_to(&mut wire);
    wire.extend_from_slice(b"short"); // only 5 bytes, not 100
    assert!(frame_unpack(&wire).is_none());
}

#[test]
fn unknown_command_dispatch() {
    let table = Router::new();
    let err = table.call("does_not_exist", vec![1, 2, 3]).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("unknown command"));
    assert!(msg.contains("does_not_exist"));
}

#[test]
fn empty_payload_dispatch() {
    let table = Router::new();
    table.register("echo", |payload: Vec<u8>| payload);

    let resp = table.call("echo", vec![]).unwrap();
    assert!(resp.is_empty());
}

#[test]
fn decode_truncated_data() {
    // u32 needs 4 bytes, give it 2
    assert!(u32::decode(&[0x01, 0x02]).is_none());

    // String with length prefix claiming 10 bytes but only 2 data bytes
    let mut buf = Vec::new();
    10u32.encode(&mut buf); // length prefix = 10
    buf.extend_from_slice(&[0x41, 0x42]); // only 2 bytes
    assert!(String::decode(&buf).is_none());
}

#[test]
fn conduit_error_variants() {
    let e1 = Error::AuthFailed;
    assert_eq!(e1.to_string(), "authentication failed");

    let e2 = Error::UnknownCommand("test_cmd".into());
    assert!(e2.to_string().contains("test_cmd"));

    let e3 = Error::DecodeFailed;
    assert_eq!(e3.to_string(), "frame decode failed");
}

#[test]
fn bool_decode_invalid_value() {
    // bool should only accept 0 or 1
    assert!(bool::decode(&[2]).is_none());
    assert!(bool::decode(&[0xFF]).is_none());
    assert!(bool::decode(&[]).is_none());
}

// ---------------------------------------------------------------------------
// 6. Router concurrent access
// ---------------------------------------------------------------------------

#[test]
fn dispatch_table_concurrent_register_and_dispatch() {
    let table = Arc::new(Router::new());

    // Seed one handler so dispatches always have something to call
    table.register("base", |payload: Vec<u8>| payload);

    let mut handles = Vec::new();

    // Spawn threads that register handlers
    for i in 0..4 {
        let t = Arc::clone(&table);
        handles.push(thread::spawn(move || {
            let name = format!("cmd_{i}");
            t.register(name, move |_payload: Vec<u8>| {
                let mut out = Vec::new();
                (i as u32).encode(&mut out);
                out
            });
        }));
    }

    // Spawn threads that dispatch concurrently
    for _ in 0..4 {
        let t = Arc::clone(&table);
        handles.push(thread::spawn(move || {
            for _ in 0..100 {
                let _ = t.call("base", b"data".to_vec());
            }
        }));
    }

    for h in handles {
        h.join().unwrap();
    }

    // All registrations should have completed
    for i in 0..4 {
        let name = format!("cmd_{i}");
        assert!(table.has(&name));

        let resp = table.call(&name, vec![]).unwrap();
        let (val, _) = u32::decode(&resp).unwrap();
        assert_eq!(val, i as u32);
    }
}

#[test]
fn dispatch_table_concurrent_dispatch_only() {
    let table = Arc::new(Router::new());

    // Register a handler that returns the payload reversed
    table.register("reverse", |mut payload: Vec<u8>| {
        payload.reverse();
        payload
    });

    let mut handles = Vec::new();
    for thread_id in 0u32..8 {
        let t = Arc::clone(&table);
        handles.push(thread::spawn(move || {
            for seq in 0u32..50 {
                let mut input = Vec::new();
                thread_id.encode(&mut input);
                seq.encode(&mut input);

                let resp = t.call("reverse", input.clone()).unwrap();

                // Reversed bytes should reverse back to original
                let mut re_reversed = resp;
                re_reversed.reverse();
                assert_eq!(re_reversed, input);
            }
        }));
    }

    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn dispatch_table_handler_replacement_under_contention() {
    let table = Arc::new(Router::new());
    table.register("contested", |_: Vec<u8>| b"v1".to_vec());

    let t1 = Arc::clone(&table);
    let t2 = Arc::clone(&table);

    // One thread replaces the handler while another dispatches
    let writer = thread::spawn(move || {
        for _ in 0..100 {
            t1.register("contested", |_: Vec<u8>| b"v2".to_vec());
        }
    });

    let reader = thread::spawn(move || {
        for _ in 0..100 {
            let resp = t2.call("contested", vec![]).unwrap();
            // Should be either v1 or v2, never corrupted
            assert!(resp == b"v1" || resp == b"v2");
        }
    });

    writer.join().unwrap();
    reader.join().unwrap();
}
