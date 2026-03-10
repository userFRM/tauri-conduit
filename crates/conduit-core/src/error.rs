//! Error types for the conduit IPC layer.
//!
//! [`Error`] covers every failure mode for the custom protocol
//! transport: authentication, serialisation, and binary framing.

use std::fmt;

/// Unified error type for all conduit operations.
#[derive(Debug)]
pub enum Error {
    /// The client failed token authentication.
    AuthFailed,
    /// JSON serialisation / deserialisation error.
    Serialize(sonic_rs::Error),
    /// An unrecognised command name was received.
    UnknownCommand(String),
    /// A binary frame could not be decoded.
    DecodeFailed,
    /// A payload exceeds the maximum encodable size (u32::MAX bytes).
    PayloadTooLarge(usize),
    /// A reliable channel's byte budget has been reached and the frame was
    /// rejected (no data was lost).
    ChannelFull,
    /// A command handler returned an application-level error.
    Handler(String),
    /// A named channel does not exist.
    UnknownChannel(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AuthFailed => f.write_str("authentication failed"),
            Self::Serialize(e) => write!(f, "serialization error: {e}"),
            Self::UnknownCommand(name) => write!(f, "unknown command: {name}"),
            Self::DecodeFailed => f.write_str("binary decode failed"),
            Self::PayloadTooLarge(len) => {
                write!(f, "payload too large: {len} bytes exceeds u32::MAX")
            }
            Self::ChannelFull => f.write_str("channel full: byte limit reached"),
            Self::Handler(msg) => write!(f, "handler error: {msg}"),
            Self::UnknownChannel(name) => write!(f, "unknown channel: {name}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Serialize(e) => Some(e),
            _ => None,
        }
    }
}

impl From<sonic_rs::Error> for Error {
    fn from(e: sonic_rs::Error) -> Self {
        Self::Serialize(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_auth_failed() {
        let err = Error::AuthFailed;
        assert_eq!(err.to_string(), "authentication failed");
    }

    #[test]
    fn display_unknown_command() {
        let err = Error::UnknownCommand("foo".into());
        assert_eq!(err.to_string(), "unknown command: foo");
    }

    #[test]
    fn from_sonic_rs() {
        let json_err = sonic_rs::from_str::<String>("not json").unwrap_err();
        let err: Error = json_err.into();
        assert!(matches!(err, Error::Serialize(_)));
    }

    #[test]
    fn error_source_none_variants() {
        assert!(std::error::Error::source(&Error::AuthFailed).is_none());
        assert!(std::error::Error::source(&Error::DecodeFailed).is_none());
        assert!(std::error::Error::source(&Error::PayloadTooLarge(0)).is_none());
    }

    #[test]
    fn display_payload_too_large() {
        let err = Error::PayloadTooLarge(5_000_000_000);
        assert_eq!(
            err.to_string(),
            "payload too large: 5000000000 bytes exceeds u32::MAX"
        );
    }

    #[test]
    fn display_decode_failed() {
        let err = Error::DecodeFailed;
        assert_eq!(err.to_string(), "binary decode failed");
    }

    #[test]
    fn display_channel_full() {
        let err = Error::ChannelFull;
        assert_eq!(err.to_string(), "channel full: byte limit reached");
    }

    #[test]
    fn error_source_channel_full() {
        assert!(std::error::Error::source(&Error::ChannelFull).is_none());
    }

    #[test]
    fn display_handler_error() {
        let err = Error::Handler("division by zero".into());
        assert_eq!(err.to_string(), "handler error: division by zero");
    }

    #[test]
    fn error_source_handler() {
        assert!(std::error::Error::source(&Error::Handler("x".into())).is_none());
    }

    #[test]
    fn display_unknown_channel() {
        let err = Error::UnknownChannel("telemetry".into());
        assert_eq!(err.to_string(), "unknown channel: telemetry");
    }

    #[test]
    fn error_source_unknown_channel() {
        assert!(std::error::Error::source(&Error::UnknownChannel("x".into())).is_none());
    }
}
