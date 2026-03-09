#![deny(missing_docs)]
//! Facade crate for tauri-conduit.
//!
//! Re-exports the `#[command]` attribute macro so users can write
//! `#[conduit::command]` — the conduit equivalent of `#[tauri::command]`.

pub use conduit_core::{Decode, Encode, Error};
pub use conduit_derive::command;
