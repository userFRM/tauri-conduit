#![deny(missing_docs)]
//! Facade crate for tauri-conduit.
//!
//! Re-exports the `#[command]` attribute macro so users can write
//! `#[tauri_conduit::command]` — the conduit equivalent of `#[tauri::command]`.

pub use conduit_core::{ConduitHandler, Decode, Encode, Error, HandlerResponse};
pub use conduit_derive::{command, handler};

#[doc(hidden)]
pub use conduit_core::serde;
