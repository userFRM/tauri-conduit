//! Command dispatch table with synchronous handlers.
//!
//! [`Router`] is a thread-safe named registry: each command name maps
//! to a boxed function that receives a payload and returns a response.
//! Handlers are synchronous — callers that need async work should use a
//! channel or spawn internally.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use crate::error::Error;

/// Boxed synchronous handler: takes payload bytes, returns response bytes.
type BoxedHandler = Box<dyn Fn(Vec<u8>) -> Vec<u8> + Send + Sync>;

/// Named command registry with synchronous dispatch.
pub struct Router {
    handlers: RwLock<HashMap<String, Arc<BoxedHandler>>>,
}

impl Router {
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
    /// [`Error::UnknownCommand`] if no handler is registered for `name`.
    pub fn call(&self, name: &str, payload: Vec<u8>) -> Result<Vec<u8>, Error> {
        let handler = {
            let handlers = self.handlers.read().unwrap_or_else(|e| e.into_inner());
            handlers.get(name).cloned()
        };
        match handler {
            Some(h) => Ok(h(payload)),
            None => Err(Error::UnknownCommand(name.to_string())),
        }
    }

    /// Dispatch a command by name, returning raw bytes in all cases.
    ///
    /// On success the handler's response bytes are returned. On failure the
    /// error's `Display` text is returned as UTF-8 bytes. This is a
    /// convenience wrapper for call sites (such as the custom protocol
    /// handler) that must always produce a `Vec<u8>`.
    #[must_use]
    pub fn call_or_error_bytes(&self, name: &str, payload: Vec<u8>) -> Vec<u8> {
        match self.call(name, payload) {
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

impl std::fmt::Debug for Router {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let count = self
            .handlers
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .len();
        f.debug_struct("Router")
            .field("handler_count", &count)
            .finish()
    }
}

impl Default for Router {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_and_dispatch() {
        let table = Router::new();
        table.register("echo", |payload: Vec<u8>| payload);
        let resp = table.call("echo", b"hello".to_vec()).unwrap();
        assert_eq!(resp, b"hello");
    }

    #[test]
    fn unknown_command() {
        let table = Router::new();
        let err = table.call("nope", vec![]).unwrap_err();
        assert!(matches!(err, Error::UnknownCommand(ref name) if name == "nope"));
        assert_eq!(err.to_string(), "unknown command: nope");
    }

    #[test]
    fn has_command() {
        let table = Router::new();
        assert!(!table.has("ping"));
        table.register("ping", |_payload: Vec<u8>| b"pong".to_vec());
        assert!(table.has("ping"));
    }

    #[test]
    fn register_simple_test() {
        let table = Router::new();
        table.register_simple("version", || b"1.0".to_vec());
        let resp = table.call("version", vec![0xFF]).unwrap();
        assert_eq!(resp, b"1.0");
    }

    #[test]
    fn call_or_error_bytes_success() {
        let table = Router::new();
        table.register("echo", |payload: Vec<u8>| payload);
        let resp = table.call_or_error_bytes("echo", b"hello".to_vec());
        assert_eq!(resp, b"hello");
    }

    #[test]
    fn call_or_error_bytes_unknown() {
        let table = Router::new();
        let resp = table.call_or_error_bytes("nope", vec![]);
        assert_eq!(resp, b"unknown command: nope");
    }
}
