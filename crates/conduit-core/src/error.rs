//! Error types for the conduit IPC layer.
//!
//! [`ConduitError`] covers every failure mode for the custom protocol
//! transport: authentication, serialisation, and binary framing.

use std::fmt;

/// Unified error type for all conduit operations.
#[derive(Debug)]
pub enum ConduitError {
    /// The client failed token authentication.
    AuthFailed,
    /// JSON serialisation / deserialisation error.
    Serialize(serde_json::Error),
    /// An unrecognised command name was received.
    UnknownCommand(String),
    /// A binary frame could not be decoded.
    DecodeFailed,
    /// A payload exceeds the maximum encodable size (u32::MAX bytes).
    PayloadTooLarge(usize),
}

impl fmt::Display for ConduitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AuthFailed => f.write_str("authentication failed"),
            Self::Serialize(e) => write!(f, "serialization error: {e}"),
            Self::UnknownCommand(name) => write!(f, "unknown command: {name}"),
            Self::DecodeFailed => f.write_str("frame decode failed"),
            Self::PayloadTooLarge(len) => {
                write!(f, "payload too large: {len} bytes exceeds u32::MAX")
            }
        }
    }
}

impl std::error::Error for ConduitError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Serialize(e) => Some(e),
            _ => None,
        }
    }
}

impl From<serde_json::Error> for ConduitError {
    fn from(e: serde_json::Error) -> Self {
        Self::Serialize(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_auth_failed() {
        let err = ConduitError::AuthFailed;
        assert_eq!(err.to_string(), "authentication failed");
    }

    #[test]
    fn display_unknown_command() {
        let err = ConduitError::UnknownCommand("foo".into());
        assert_eq!(err.to_string(), "unknown command: foo");
    }

    #[test]
    fn from_serde_json() {
        let json_err = serde_json::from_str::<String>("not json").unwrap_err();
        let err: ConduitError = json_err.into();
        assert!(matches!(err, ConduitError::Serialize(_)));
    }

    #[test]
    fn error_source_none_variants() {
        assert!(std::error::Error::source(&ConduitError::AuthFailed).is_none());
        assert!(std::error::Error::source(&ConduitError::DecodeFailed).is_none());
        assert!(std::error::Error::source(&ConduitError::PayloadTooLarge(0)).is_none());
    }

    #[test]
    fn display_payload_too_large() {
        let err = ConduitError::PayloadTooLarge(5_000_000_000);
        assert_eq!(
            err.to_string(),
            "payload too large: 5000000000 bytes exceeds u32::MAX"
        );
    }
}
