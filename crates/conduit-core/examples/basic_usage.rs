//! Basic usage example for conduit-core.
//!
//! Demonstrates the full conduit flow without any Tauri dependency:
//!
//! 1. Creating a Router and registering handlers
//! 2. Building a request frame (FrameHeader + payload)
//! 3. Dispatching through the table
//! 4. Wrapping the response in a frame
//! 5. Using the ring buffer to push/drain data
//! 6. Parsing the drain wire format
//!
//! Run with: cargo run --example basic_usage -p conduit-core

use conduit_core::{
    Decode, Encode, FRAME_HEADER_SIZE, FrameHeader, MsgType, PROTOCOL_VERSION, RingBuffer, Router,
    frame_pack, frame_unpack,
};

fn main() {
    println!("=== conduit-core basic usage ===\n");

    // -----------------------------------------------------------------------
    // Step 1: Create a Router and register handlers
    // -----------------------------------------------------------------------
    println!("--- Step 1: Router ---\n");

    let table = Router::new();

    // Register an "echo" handler that returns the payload unchanged.
    table.register("echo", |payload: Vec<u8>| payload);

    // Register a "greet" handler that decodes a name string from the payload
    // and returns a greeting.
    table.register("greet", |payload: Vec<u8>| match String::decode(&payload) {
        Some((name, _consumed)) => {
            let greeting = format!("Hello, {name}! Welcome to conduit.");
            let mut out = Vec::new();
            greeting.encode(&mut out);
            out
        }
        None => b"decode error".to_vec(),
    });

    // Register a simple handler that takes no payload.
    table.register_simple("version", || {
        let mut out = Vec::new();
        String::from("conduit-core 1.0.0").encode(&mut out);
        out
    });

    println!("  Registered commands: echo, greet, version");
    println!("  has(\"echo\")    = {}", table.has("echo"));
    println!("  has(\"greet\")   = {}", table.has("greet"));
    println!("  has(\"version\") = {}", table.has("version"));
    println!("  has(\"nope\")    = {}", table.has("nope"));

    // -----------------------------------------------------------------------
    // Step 2: Build a request frame
    // -----------------------------------------------------------------------
    println!("\n--- Step 2: Build a request frame ---\n");

    // Encode the payload: the name "Alice" as a wire-encoded String.
    let mut payload = Vec::new();
    String::from("Alice").encode(&mut payload);

    println!(
        "  Payload (wire-encoded \"Alice\"): {} bytes",
        payload.len()
    );
    println!("  Raw bytes: {:02X?}", payload);

    // Build the frame header.
    let request_header = FrameHeader {
        version: PROTOCOL_VERSION,
        reserved: 0,
        msg_type: MsgType::Request,
        sequence: 1,
        payload_len: payload.len() as u32,
    };

    println!("  Frame header: {:?}", request_header);

    // Wrap header + payload into a complete frame.
    let request_frame = frame_pack(&request_header, &payload);
    println!(
        "  Complete frame: {} bytes (header {FRAME_HEADER_SIZE} + payload {})",
        request_frame.len(),
        payload.len()
    );

    // -----------------------------------------------------------------------
    // Step 3: Dispatch through the table
    // -----------------------------------------------------------------------
    println!("\n--- Step 3: Dispatch the command ---\n");

    // In a real app the command name comes from the URL path. Here we call
    // the "greet" handler directly, passing just the payload bytes.
    let (parsed_header, parsed_payload) =
        frame_unpack(&request_frame).expect("frame_unpack failed");
    println!("  Parsed header: {:?}", parsed_header);
    println!("  Parsed payload length: {} bytes", parsed_payload.len());

    let response_bytes = table
        .call("greet", parsed_payload.to_vec())
        .expect("dispatch failed");

    // Decode the response to see the greeting.
    let (greeting, _) = String::decode(&response_bytes).unwrap();
    println!("  Response: \"{greeting}\"");

    // Also test error handling for unknown commands.
    let err = table.call("nonexistent", vec![]).unwrap_err();
    println!("  Unknown command error: {err}");

    // -----------------------------------------------------------------------
    // Step 4: Wrap the response in a frame
    // -----------------------------------------------------------------------
    println!("\n--- Step 4: Wrap the response in a frame ---\n");

    let response_header = FrameHeader {
        version: PROTOCOL_VERSION,
        reserved: 0,
        msg_type: MsgType::Response,
        sequence: 1, // matches the request sequence
        payload_len: response_bytes.len() as u32,
    };

    let response_frame = frame_pack(&response_header, &response_bytes);
    println!("  Response frame: {} bytes total", response_frame.len());

    // Verify roundtrip: unwrap the response frame.
    let (resp_hdr, resp_payload) = frame_unpack(&response_frame).unwrap();
    assert_eq!(resp_hdr.msg_type, MsgType::Response);
    assert_eq!(resp_hdr.sequence, 1);
    let (decoded_greeting, _) = String::decode(resp_payload).unwrap();
    println!("  Verified roundtrip: \"{decoded_greeting}\"");

    // -----------------------------------------------------------------------
    // Step 5: Ring buffer push/drain
    // -----------------------------------------------------------------------
    println!("\n--- Step 5: Ring buffer push/drain ---\n");

    // Create a ring buffer with 1 KB capacity.
    let ring = RingBuffer::new(1024);
    println!("  Created ring buffer: capacity={} bytes", ring.capacity());

    // Simulate pushing several "market tick" frames into the ring buffer.
    // In a real app these would be pushed by a backend data source and
    // drained by the custom protocol handler on each frontend poll.
    for i in 0u32..5 {
        // Build a small frame for each tick.
        let mut tick_payload = Vec::new();
        i.encode(&mut tick_payload);
        (100.0 + i as f64).encode(&mut tick_payload);

        let tick_header = FrameHeader {
            version: PROTOCOL_VERSION,
            reserved: 0,
            msg_type: MsgType::Push,
            sequence: i,
            payload_len: tick_payload.len() as u32,
        };

        let tick_frame = frame_pack(&tick_header, &tick_payload);
        let dropped = ring.push(&tick_frame);
        println!(
            "  Pushed tick {i}: {} bytes, dropped={dropped}",
            tick_frame.len()
        );
    }

    println!(
        "  Buffer state: {} frames, {} bytes used",
        ring.frame_count(),
        ring.bytes_used()
    );

    // Drain everything into a single binary blob (this is what the custom
    // protocol handler returns to the frontend).
    let blob = ring.drain_all();
    println!("  Drained blob: {} bytes", blob.len());
    println!(
        "  Buffer after drain: {} frames, {} bytes used",
        ring.frame_count(),
        ring.bytes_used()
    );

    // -----------------------------------------------------------------------
    // Step 6: Parse the drain wire format
    // -----------------------------------------------------------------------
    println!("\n--- Step 6: Parse drain wire format ---\n");

    // Wire format:
    //   [u32 LE frame_count]
    //   [u32 LE len_1][bytes_1]
    //   [u32 LE len_2][bytes_2]
    //   ...
    let frame_count = u32::from_le_bytes(blob[0..4].try_into().unwrap());
    println!("  Frame count: {frame_count}");

    let mut offset = 4usize;
    for i in 0..frame_count {
        // Read the length prefix for this frame.
        let frame_len = u32::from_le_bytes(blob[offset..offset + 4].try_into().unwrap()) as usize;
        offset += 4;

        // Extract the raw frame bytes.
        let frame_bytes = &blob[offset..offset + frame_len];
        offset += frame_len;

        // Parse the conduit frame header + payload.
        let (hdr, payload) = frame_unpack(frame_bytes).expect("bad frame in drain blob");

        // Decode the tick payload: u32 index + f64 price.
        let (index, consumed) = u32::decode(payload).unwrap();
        let (price, _) = f64::decode(&payload[consumed..]).unwrap();

        println!(
            "  Frame {i}: seq={}, type={:?}, index={index}, price={price:.1}",
            hdr.sequence, hdr.msg_type,
        );
    }

    assert_eq!(offset, blob.len(), "all bytes consumed");

    println!("\n=== Done! ===");
}
