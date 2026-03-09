//! Command dispatch table with synchronous handlers.
//!
//! [`DispatchTable`] is a thread-safe named registry: each command name maps
//! to a boxed function that receives a payload and returns a response.
//! Handlers are synchronous — callers that need async work should use a
//! channel or spawn internally.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use crate::error::ConduitError;

/// Boxed synchronous handler: takes payload bytes, returns response bytes.
type BoxedHandler = Box<dyn Fn(Vec<u8>) -> Vec<u8> + Send + Sync>;

/// Named command registry with synchronous dispatch.
pub struct DispatchTable {
    handlers: RwLock<HashMap<String, Arc<BoxedHandler>>>,
}

impl DispatchTable {
    /// Create an empty dispatch table.
    pub fn new() -> Self {
        Self {
            handlers: RwLock::new(HashMap::new()),
        }
    }

    /// Register a handler for a command name.
    ///
    /// If a handler was already registered under `name` it is replaced.
    pub fn register<F>(&self, name: impl Into<String>, handler: F)
    where
        F: Fn(Vec<u8>) -> Vec<u8> + Send + Sync + 'static,
    {
        let boxed: BoxedHandler = Box::new(handler);
        self.handlers
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .insert(name.into(), Arc::new(boxed));
    }

    /// Register a handler that takes no payload.
    ///
    /// The incoming payload bytes are silently discarded.
    pub fn register_simple<F>(&self, name: impl Into<String>, handler: F)
    where
        F: Fn() -> Vec<u8> + Send + Sync + 'static,
    {
        let boxed: BoxedHandler = Box::new(move |_payload| handler());
        self.handlers
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .insert(name.into(), Arc::new(boxed));
    }

    /// Dispatch a command by name.
    ///
    /// Returns the handler's response bytes on success, or
    /// [`ConduitError::UnknownCommand`] if no handler is registered for `name`.
    pub fn dispatch(&self, name: &str, payload: Vec<u8>) -> Result<Vec<u8>, ConduitError> {
        let handler = {
            let handlers = self.handlers.read().unwrap_or_else(|e| e.into_inner());
            handlers.get(name).cloned()
        };
        match handler {
            Some(h) => Ok(h(payload)),
            None => Err(ConduitError::UnknownCommand(name.to_string())),
        }
    }

    /// Dispatch a command by name, returning raw bytes in all cases.
    ///
    /// On success the handler's response bytes are returned. On failure the
    /// error's `Display` text is returned as UTF-8 bytes. This is a
    /// convenience wrapper for call sites (such as the custom protocol
    /// handler) that must always produce a `Vec<u8>`.
    #[must_use]
    pub fn dispatch_or_error_bytes(&self, name: &str, payload: Vec<u8>) -> Vec<u8> {
        match self.dispatch(name, payload) {
            Ok(bytes) => bytes,
            Err(e) => e.to_string().into_bytes(),
        }
    }

    /// Check whether a command is registered.
    #[must_use]
    pub fn has(&self, name: &str) -> bool {
        self.handlers
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .contains_key(name)
    }
}

impl std::fmt::Debug for DispatchTable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let count = self
            .handlers
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .len();
        f.debug_struct("DispatchTable")
            .field("handler_count", &count)
            .finish()
    }
}

impl Default for DispatchTable {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_and_dispatch() {
        let table = DispatchTable::new();
        table.register("echo", |payload: Vec<u8>| payload);
        let resp = table.dispatch("echo", b"hello".to_vec()).unwrap();
        assert_eq!(resp, b"hello");
    }

    #[test]
    fn unknown_command() {
        let table = DispatchTable::new();
        let err = table.dispatch("nope", vec![]).unwrap_err();
        assert!(matches!(err, ConduitError::UnknownCommand(ref name) if name == "nope"));
        assert_eq!(err.to_string(), "unknown command: nope");
    }

    #[test]
    fn has_command() {
        let table = DispatchTable::new();
        assert!(!table.has("ping"));
        table.register("ping", |_payload: Vec<u8>| b"pong".to_vec());
        assert!(table.has("ping"));
    }

    #[test]
    fn register_simple_test() {
        let table = DispatchTable::new();
        table.register_simple("version", || b"1.0".to_vec());
        let resp = table.dispatch("version", vec![0xFF]).unwrap();
        assert_eq!(resp, b"1.0");
    }

    #[test]
    fn dispatch_or_error_bytes_success() {
        let table = DispatchTable::new();
        table.register("echo", |payload: Vec<u8>| payload);
        let resp = table.dispatch_or_error_bytes("echo", b"hello".to_vec());
        assert_eq!(resp, b"hello");
    }

    #[test]
    fn dispatch_or_error_bytes_unknown() {
        let table = DispatchTable::new();
        let resp = table.dispatch_or_error_bytes("nope", vec![]);
        assert_eq!(resp, b"unknown command: nope");
    }
}
