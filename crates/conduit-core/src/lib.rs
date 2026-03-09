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
    Decode, Encode, FRAME_HEADER_SIZE, FrameHeader, MsgType, PROTOCOL_VERSION, frame_pack,
    frame_unpack,
};
pub use error::Error;
pub use ringbuf::RingBuffer;
pub use router::Router;
