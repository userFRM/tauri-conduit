#![forbid(unsafe_code)]
#![deny(missing_docs)]
//! # conduit-core
//!
//! Binary IPC core for Tauri v2. Provides a binary codec, synchronous dispatch
//! table, and in-process ring buffer for the `conduit://` custom protocol.
//!
//! ## Dependency note
//!
//! This crate depends on `serde` and `sonic-rs` unconditionally. These are
//! required by the [`Router`] JSON handler methods, the [`ConduitHandler`]
//! trait (which powers `#[conduit::command]`), and the [`Error::Serialize`]
//! variant. The pure binary codec ([`Encode`]/[`Decode`], [`RingBuffer`],
//! [`Queue`]) does not use JSON at runtime, but the types are not
//! feature-gated because the handler system is considered a core part of
//! conduit's purpose.

pub mod channel;
pub mod codec;
pub mod error;
pub mod handler;
pub mod queue;
pub mod ringbuf;
pub mod router;

pub use channel::ChannelBuffer;
pub use codec::{
    Bytes, DRAIN_FRAME_OVERHEAD, Decode, Encode, FRAME_HEADER_SIZE, FrameHeader, MsgType,
    PROTOCOL_VERSION, frame_pack, frame_unpack,
};
pub use error::Error;
pub use handler::{ConduitHandler, HandlerContext, HandlerResponse};
pub use queue::Queue;
pub use ringbuf::{PushOutcome, RingBuffer};
pub use router::Router;

/// Acquire a [`Mutex`](std::sync::Mutex) lock, recovering from poison if
/// another thread panicked while holding it.
///
/// Conduit's buffers are designed to remain usable after a panic — the data
/// in a poisoned mutex is still valid. This helper avoids copy-pasting the
/// `.unwrap_or_else(|e| e.into_inner())` pattern.
#[inline]
pub fn lock_or_recover<T>(mutex: &std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

/// Acquire a [`RwLock`](std::sync::RwLock) write guard, recovering from
/// poison.
#[inline]
pub fn write_or_recover<T>(lock: &std::sync::RwLock<T>) -> std::sync::RwLockWriteGuard<'_, T> {
    lock.write().unwrap_or_else(|e| e.into_inner())
}

/// Acquire a [`RwLock`](std::sync::RwLock) read guard, recovering from
/// poison.
#[inline]
pub fn read_or_recover<T>(lock: &std::sync::RwLock<T>) -> std::sync::RwLockReadGuard<'_, T> {
    lock.read().unwrap_or_else(|e| e.into_inner())
}

#[doc(hidden)]
pub use serde;

#[doc(hidden)]
pub use sonic_rs;
