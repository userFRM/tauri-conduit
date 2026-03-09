//! Derive macro usage example for conduit-core + conduit-derive.
//!
//! Demonstrates Encode/Decode derive macros for struct serialization:
//!
//! 1. Define a MarketTick struct with derive macros
//! 2. Encode it and show the byte count
//! 3. Decode it back and verify the roundtrip
//! 4. Embed the encoded struct in a conduit frame
//!
//! Run with: cargo run --example derive_usage -p conduit-core

use conduit_core::{
    Decode, Encode, FRAME_HEADER_SIZE, FrameHeader, MsgType, PROTOCOL_VERSION, frame_pack,
    frame_unpack,
};
use conduit_derive::{Decode, Encode};

// ---------------------------------------------------------------------------
// Step 1: Define structs with derive macros
// ---------------------------------------------------------------------------

/// A market data tick -- the kind of struct you'd stream at high frequency
/// through the ring buffer.
///
/// The derive macros generate `Encode` and `Decode` impls that
/// encode/decode each field in declaration order using little-endian binary.
#[derive(Debug, PartialEq, Encode, Decode)]
struct MarketTick {
    /// Unix timestamp in microseconds.
    timestamp: i64,
    /// Price as a 64-bit float.
    price: f64,
    /// Volume as a 64-bit float.
    volume: f64,
    /// Trade side: 0 = buy, 1 = sell.
    side: u8,
}

/// An order book level with variable-length symbol name.
#[derive(Debug, PartialEq, Encode, Decode)]
struct OrderBookLevel {
    /// Trading pair symbol (variable-length, 4-byte length prefix on wire).
    symbol: String,
    /// Bid price.
    bid: f64,
    /// Ask price.
    ask: f64,
    /// Bid size.
    bid_size: f64,
    /// Ask size.
    ask_size: f64,
    /// Level depth (0 = best bid/ask).
    depth: u32,
}

fn main() {
    println!("=== conduit-derive usage ===\n");

    // -----------------------------------------------------------------------
    // Step 2: Encode a MarketTick and show the byte count
    // -----------------------------------------------------------------------
    println!("--- Step 2: Encode a MarketTick ---\n");

    let tick = MarketTick {
        timestamp: 1_700_000_000_000_000, // microsecond precision
        price: 42_567.89,
        volume: 1.2345,
        side: 1, // sell
    };

    println!("  Original: {tick:?}");

    // encode_size() tells you the exact byte count before encoding.
    let predicted_size = tick.encode_size();
    println!("  Predicted wire size: {predicted_size} bytes");
    println!("    i64 (timestamp) = 8 bytes");
    println!("    f64 (price)     = 8 bytes");
    println!("    f64 (volume)    = 8 bytes");
    println!("    u8  (side)      = 1 byte");
    println!("    Total           = 25 bytes");
    assert_eq!(predicted_size, 25);

    // Encode to bytes.
    let mut buf = Vec::new();
    tick.encode(&mut buf);
    println!("  Encoded: {} bytes", buf.len());
    println!("  Raw hex: {:02X?}", buf);
    assert_eq!(buf.len(), predicted_size);

    // -----------------------------------------------------------------------
    // Step 3: Decode it back and verify the roundtrip
    // -----------------------------------------------------------------------
    println!("\n--- Step 3: Decode and verify roundtrip ---\n");

    let (decoded, consumed) = MarketTick::decode(&buf).unwrap();
    println!("  Decoded: {decoded:?}");
    println!("  Bytes consumed: {consumed}");
    assert_eq!(decoded, tick);
    assert_eq!(consumed, buf.len());
    println!("  Roundtrip verified: original == decoded");

    // -----------------------------------------------------------------------
    // Step 4: Embed in a conduit frame
    // -----------------------------------------------------------------------
    println!("\n--- Step 4: Embed in a conduit frame ---\n");

    let header = FrameHeader {
        version: PROTOCOL_VERSION,
        reserved: 0,
        msg_type: MsgType::Push,
        sequence: 42,
        payload_len: buf.len() as u32,
    };

    let frame = frame_pack(&header, &buf);
    println!(
        "  Frame: {} bytes (header {} + payload {})",
        frame.len(),
        FRAME_HEADER_SIZE,
        buf.len()
    );

    // Unwrap the frame and decode the MarketTick from the payload.
    let (parsed_header, payload) = frame_unpack(&frame).expect("frame_unpack failed");
    let (parsed_tick, _) = MarketTick::decode(payload).expect("decode failed");

    assert_eq!(parsed_header.msg_type, MsgType::Push);
    assert_eq!(parsed_header.sequence, 42);
    assert_eq!(parsed_tick, tick);
    println!("  Frame roundtrip verified!");
    println!("    header.msg_type = {:?}", parsed_header.msg_type);
    println!("    header.sequence = {}", parsed_header.sequence);
    println!("    payload tick    = {:?}", parsed_tick);

    // -----------------------------------------------------------------------
    // Bonus: Variable-length struct (OrderBookLevel)
    // -----------------------------------------------------------------------
    println!("\n--- Bonus: Variable-length struct ---\n");

    let level = OrderBookLevel {
        symbol: String::from("BTC/USD"),
        bid: 42_500.00,
        ask: 42_501.50,
        bid_size: 3.5,
        ask_size: 1.2,
        depth: 0,
    };

    println!("  Original: {level:?}");

    let size = level.encode_size();
    println!("  Wire size: {size} bytes");
    println!("    String \"BTC/USD\" = 4 (length prefix) + 7 (UTF-8) = 11 bytes");
    println!("    f64 x4           = 32 bytes");
    println!("    u32 (depth)      = 4 bytes");
    println!("    Total            = 47 bytes");
    assert_eq!(size, 47);

    let mut level_buf = Vec::new();
    level.encode(&mut level_buf);

    let (decoded_level, consumed) = OrderBookLevel::decode(&level_buf).unwrap();
    assert_eq!(decoded_level, level);
    assert_eq!(consumed, level_buf.len());
    println!("  Roundtrip verified: {} bytes consumed", consumed);

    // -----------------------------------------------------------------------
    // Bonus: Multiple structs back-to-back in one buffer
    // -----------------------------------------------------------------------
    println!("\n--- Bonus: Back-to-back encoding ---\n");

    let mut combined = Vec::new();
    tick.encode(&mut combined);
    level.encode(&mut combined);
    println!(
        "  Combined buffer: {} bytes (tick {} + level {})",
        combined.len(),
        tick.encode_size(),
        level.encode_size()
    );

    // Decode them sequentially.
    let (t, t_len) = MarketTick::decode(&combined).unwrap();
    let (l, l_len) = OrderBookLevel::decode(&combined[t_len..]).unwrap();
    assert_eq!(t, tick);
    assert_eq!(l, level);
    assert_eq!(t_len + l_len, combined.len());
    println!("  Both decoded successfully, all bytes consumed.");

    println!("\n=== Done! ===");
}
