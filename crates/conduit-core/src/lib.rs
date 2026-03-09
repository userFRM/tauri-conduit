#![forbid(unsafe_code)]
#![deny(missing_docs)]
//! # conduit-core
//!
//! Binary IPC core for Tauri v2. Provides a binary codec, synchronous dispatch
//! table, and in-process ring buffer for the `conduit://` custom protocol.

pub mod codec;
pub mod error;
pub mod ringbuf;
pub mod router;

pub use codec::{
    FRAME_HEADER_SIZE, FrameHeader, MsgType, PROTOCOL_VERSION, WireDecode, WireEncode,
    frame_unwrap, frame_wrap,
};
pub use error::ConduitError;
pub use ringbuf::ConduitRingBuffer;
pub use router::DispatchTable;
