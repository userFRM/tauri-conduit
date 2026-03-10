//! Command dispatch table with synchronous handlers.
//!
//! [`Router`] is a thread-safe named registry: each command name maps
//! to a boxed function that receives a payload and returns a response.
//! Handlers are synchronous — callers that need async work should use a
//! channel or spawn internally.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::codec::{Decode, Encode};
use crate::error::Error;

/// Boxed synchronous handler: takes payload bytes and an opaque context,
/// returns response bytes or an [`Error`].
///
/// The context parameter (`&dyn std::any::Any`) allows handlers generated
/// by the `#[command]` macro to extract `State<T>` from an `AppHandle`.
/// Existing handler registration methods ignore the context parameter for
/// backward compatibility.
type BoxedHandler =
    Box<dyn Fn(Vec<u8>, &dyn std::any::Any) -> Result<Vec<u8>, Error> + Send + Sync>;

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
        let boxed: BoxedHandler = Box::new(move |payload, _ctx| Ok(handler(payload)));
        crate::write_or_recover(&self.handlers).insert(name.into(), Arc::new(boxed));
    }

    /// Register a handler that takes no payload.
    ///
    /// The incoming payload bytes are silently discarded.
    pub fn register_simple<F>(&self, name: impl Into<String>, handler: F)
    where
        F: Fn() -> Vec<u8> + Send + Sync + 'static,
    {
        let boxed: BoxedHandler = Box::new(move |_payload, _ctx| Ok(handler()));
        crate::write_or_recover(&self.handlers).insert(name.into(), Arc::new(boxed));
    }

    /// Register a JSON handler for a command name.
    ///
    /// The incoming payload is deserialised from JSON into `A`, the handler
    /// is called with the typed value, and the return value `R` is serialised
    /// back to JSON bytes. Returns [`Error::Serialize`] on deserialisation
    /// failure.
    pub fn register_json<F, A, R>(&self, name: impl Into<String>, handler: F)
    where
        F: Fn(A) -> R + Send + Sync + 'static,
        A: DeserializeOwned + 'static,
        R: Serialize + 'static,
    {
        let boxed: BoxedHandler = Box::new(move |payload, _ctx| {
            let arg: A = sonic_rs::from_slice(&payload).map_err(Error::from)?;
            let result = handler(arg);
            sonic_rs::to_vec(&result).map_err(Error::from)
        });
        crate::write_or_recover(&self.handlers).insert(name.into(), Arc::new(boxed));
    }

    /// Register a fallible JSON handler for a command name.
    ///
    /// Like [`register_json`](Self::register_json), but the handler returns
    /// `Result<R, E>`. On `Ok(value)`, the value is serialised to JSON. On
    /// `Err(e)`, the error's `Display` text is returned as
    /// [`Error::Handler`].
    pub fn register_json_result<F, A, R, E>(&self, name: impl Into<String>, handler: F)
    where
        F: Fn(A) -> Result<R, E> + Send + Sync + 'static,
        A: DeserializeOwned + 'static,
        R: Serialize + 'static,
        E: std::fmt::Display + 'static,
    {
        let boxed: BoxedHandler = Box::new(move |payload, _ctx| {
            let arg: A = sonic_rs::from_slice(&payload).map_err(Error::from)?;
            let result = handler(arg).map_err(|e| Error::Handler(e.to_string()))?;
            sonic_rs::to_vec(&result).map_err(Error::from)
        });
        crate::write_or_recover(&self.handlers).insert(name.into(), Arc::new(boxed));
    }

    /// Register a binary handler for a command name.
    ///
    /// The incoming payload is decoded via the [`Decode`] trait into `A`,
    /// the handler is called with the typed value, and the return value `R`
    /// is encoded via [`Encode`] back to bytes. Returns
    /// [`Error::DecodeFailed`] if the payload cannot be decoded.
    pub fn register_binary<F, A, R>(&self, name: impl Into<String>, handler: F)
    where
        F: Fn(A) -> R + Send + Sync + 'static,
        A: Decode + 'static,
        R: Encode + 'static,
    {
        let boxed: BoxedHandler = Box::new(move |payload, _ctx| {
            let (arg, _consumed) = A::decode(&payload).ok_or(Error::DecodeFailed)?;
            let result = handler(arg);
            let mut buf = Vec::with_capacity(result.encode_size());
            result.encode(&mut buf);
            Ok(buf)
        });
        crate::write_or_recover(&self.handlers).insert(name.into(), Arc::new(boxed));
    }

    /// Register a context-aware handler.
    ///
    /// Handlers generated by the `#[conduit::command]` macro have the
    /// signature `fn(Vec<u8>, &dyn Any) -> Result<Vec<u8>, Error>` and
    /// handle their own deserialization, State extraction, and
    /// serialization internally.
    pub fn register_with_context<F>(&self, name: impl Into<String>, handler: F)
    where
        F: Fn(Vec<u8>, &dyn std::any::Any) -> Result<Vec<u8>, Error> + Send + Sync + 'static,
    {
        let boxed: BoxedHandler = Box::new(handler);
        crate::write_or_recover(&self.handlers).insert(name.into(), Arc::new(boxed));
    }

    /// Dispatch a command by name with an opaque context.
    ///
    /// The context is passed through to the handler. For handlers
    /// registered via `register_with_context` (i.e., `#[command]`-generated
    /// handlers), the context is typically an `&AppHandle<Wry>` that enables
    /// `State<T>` extraction.
    pub fn call_with_context(
        &self,
        name: &str,
        payload: Vec<u8>,
        ctx: &dyn std::any::Any,
    ) -> Result<Vec<u8>, Error> {
        let handler = {
            let handlers = crate::read_or_recover(&self.handlers);
            handlers.get(name).cloned()
        };
        match handler {
            Some(h) => h(payload, ctx),
            None => Err(Error::UnknownCommand(name.to_string())),
        }
    }

    /// Dispatch a command by name with context, returning raw bytes in all
    /// cases.
    ///
    /// On success the handler's response bytes are returned. On failure the
    /// error's `Display` text is returned as UTF-8 bytes.
    #[must_use]
    pub fn call_or_error_bytes_with_context(
        &self,
        name: &str,
        payload: Vec<u8>,
        ctx: &dyn std::any::Any,
    ) -> Vec<u8> {
        match self.call_with_context(name, payload, ctx) {
            Ok(bytes) => bytes,
            Err(e) => e.to_string().into_bytes(),
        }
    }

    /// Dispatch a command by name.
    ///
    /// Returns the handler's response bytes on success, or
    /// [`Error::UnknownCommand`] if no handler is registered for `name`.
    pub fn call(&self, name: &str, payload: Vec<u8>) -> Result<Vec<u8>, Error> {
        self.call_with_context(name, payload, &())
    }

    /// Dispatch a command by name, returning raw bytes in all cases.
    ///
    /// On success the handler's response bytes are returned. On failure the
    /// error's `Display` text is returned as UTF-8 bytes. This is a
    /// convenience wrapper for call sites (such as the custom protocol
    /// handler) that must always produce a `Vec<u8>`.
    #[must_use]
    pub fn call_or_error_bytes(&self, name: &str, payload: Vec<u8>) -> Vec<u8> {
        self.call_or_error_bytes_with_context(name, payload, &())
    }

    /// Check whether a command is registered.
    #[must_use]
    pub fn has(&self, name: &str) -> bool {
        crate::read_or_recover(&self.handlers).contains_key(name)
    }
}

impl std::fmt::Debug for Router {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let count = crate::read_or_recover(&self.handlers).len();
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

    // -- JSON handler tests --------------------------------------------------

    #[test]
    fn register_json_roundtrip() {
        let table = Router::new();
        table.register_json("add", |args: (i32, i32)| args.0 + args.1);
        let payload = sonic_rs::to_vec(&(3, 4)).unwrap();
        let resp = table.call("add", payload).unwrap();
        let result: i32 = sonic_rs::from_slice(&resp).unwrap();
        assert_eq!(result, 7);
    }

    #[test]
    fn register_json_bad_input() {
        let table = Router::new();
        table.register_json("add", |args: (i32, i32)| args.0 + args.1);
        let err = table.call("add", b"not json!".to_vec()).unwrap_err();
        assert!(matches!(err, Error::Serialize(_)));
    }

    // -- Fallible JSON handler tests -----------------------------------------

    #[test]
    fn register_json_result_ok() {
        let table = Router::new();
        table.register_json_result("divide", |args: (f64, f64)| -> Result<f64, String> {
            if args.1 == 0.0 {
                Err("division by zero".into())
            } else {
                Ok(args.0 / args.1)
            }
        });
        let payload = sonic_rs::to_vec(&(10.0_f64, 2.0_f64)).unwrap();
        let resp = table.call("divide", payload).unwrap();
        let result: f64 = sonic_rs::from_slice(&resp).unwrap();
        assert!((result - 5.0).abs() < f64::EPSILON);
    }

    #[test]
    fn register_json_result_err() {
        let table = Router::new();
        table.register_json_result("divide", |args: (f64, f64)| -> Result<f64, String> {
            if args.1 == 0.0 {
                Err("division by zero".into())
            } else {
                Ok(args.0 / args.1)
            }
        });
        let payload = sonic_rs::to_vec(&(10.0_f64, 0.0_f64)).unwrap();
        let err = table.call("divide", payload).unwrap_err();
        assert!(matches!(err, Error::Handler(ref msg) if msg == "division by zero"));
    }

    #[test]
    fn register_json_result_bad_input() {
        let table = Router::new();
        table.register_json_result("divide", |args: (f64, f64)| -> Result<f64, String> {
            Ok(args.0 / args.1)
        });
        let err = table.call("divide", b"garbage".to_vec()).unwrap_err();
        assert!(matches!(err, Error::Serialize(_)));
    }

    // -- Binary handler tests ------------------------------------------------

    /// Minimal newtype that implements Encode/Decode for testing.
    #[derive(Debug, PartialEq)]
    struct Pair(u32, u32);

    impl crate::codec::Encode for Pair {
        fn encode(&self, buf: &mut Vec<u8>) {
            self.0.encode(buf);
            self.1.encode(buf);
        }
        fn encode_size(&self) -> usize {
            8
        }
    }

    impl crate::codec::Decode for Pair {
        fn decode(data: &[u8]) -> Option<(Self, usize)> {
            let (a, ca) = u32::decode(data)?;
            let (b, cb) = u32::decode(&data[ca..])?;
            Some((Pair(a, b), ca + cb))
        }
    }

    #[test]
    fn register_binary_roundtrip() {
        let table = Router::new();
        table.register_binary("sum", |p: Pair| p.0 + p.1);
        let mut payload = Vec::new();
        Pair(10, 20).encode(&mut payload);
        let resp = table.call("sum", payload).unwrap();
        let (result, _) = u32::decode(&resp).unwrap();
        assert_eq!(result, 30);
    }

    #[test]
    fn register_binary_bad_input() {
        let table = Router::new();
        table.register_binary("sum", |p: Pair| p.0 + p.1);
        // Only 3 bytes — too short for two u32 values.
        let err = table.call("sum", vec![1, 2, 3]).unwrap_err();
        assert!(matches!(err, Error::DecodeFailed));
    }

    // -- Context-aware handler tests -----------------------------------------

    #[test]
    fn register_with_context_basic() {
        let table = Router::new();
        table.register_with_context("echo_ctx", |payload: Vec<u8>, _ctx: &dyn std::any::Any| {
            Ok(payload)
        });
        let resp = table.call("echo_ctx", b"hello".to_vec()).unwrap();
        assert_eq!(resp, b"hello");
    }

    #[test]
    fn call_with_context_passes_through() {
        let table = Router::new();
        table.register_with_context("check_ctx", |_payload: Vec<u8>, ctx: &dyn std::any::Any| {
            // Check that we can downcast the context
            if ctx.downcast_ref::<String>().is_some() {
                Ok(b"got string".to_vec())
            } else {
                Ok(b"no string".to_vec())
            }
        });

        let ctx = String::from("hello");
        let resp = table.call_with_context("check_ctx", vec![], &ctx).unwrap();
        assert_eq!(resp, b"got string");

        // call() passes &() as context, so downcast to String fails
        let resp = table.call("check_ctx", vec![]).unwrap();
        assert_eq!(resp, b"no string");
    }

    #[test]
    fn call_or_error_bytes_with_context_success() {
        let table = Router::new();
        table.register("echo", |payload: Vec<u8>| payload);
        let ctx = String::from("unused");
        let resp = table.call_or_error_bytes_with_context("echo", b"hello".to_vec(), &ctx);
        assert_eq!(resp, b"hello");
    }

    #[test]
    fn call_or_error_bytes_with_context_unknown() {
        let table = Router::new();
        let resp = table.call_or_error_bytes_with_context("nope", vec![], &());
        assert_eq!(resp, b"unknown command: nope");
    }

    #[test]
    fn register_replaces_handler() {
        let table = Router::new();
        table.register("cmd", |_payload: Vec<u8>| b"first".to_vec());
        table.register("cmd", |_payload: Vec<u8>| b"second".to_vec());
        let resp = table.call("cmd", vec![]).unwrap();
        assert_eq!(resp, b"second");
    }

    #[test]
    fn register_with_context_error_propagation() {
        let table = Router::new();
        table.register_with_context("fail", |_payload: Vec<u8>, _ctx: &dyn std::any::Any| {
            Err(Error::Handler("context handler failed".into()))
        });
        let err = table.call("fail", vec![]).unwrap_err();
        assert!(matches!(err, Error::Handler(ref msg) if msg == "context handler failed"));

        // Also verify error bytes path
        let bytes = table.call_or_error_bytes("fail", vec![]);
        assert_eq!(bytes, b"handler error: context handler failed");
    }
}
