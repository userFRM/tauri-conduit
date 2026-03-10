//! Unified channel buffer that wraps either a lossy or ordered strategy.
//!
//! [`ChannelBuffer`] is an enum over [`crate::RingBuffer`] (lossy) and
//! [`crate::Queue`] (guaranteed delivery). It provides a single
//! API surface so that the plugin layer can treat all channels uniformly.

use crate::error::Error;
use crate::queue::Queue;
use crate::ringbuf::{PushOutcome, RingBuffer};

// ---------------------------------------------------------------------------
// ChannelBuffer
// ---------------------------------------------------------------------------

/// A channel buffer that is either lossy ([`RingBuffer`]) or reliable
/// ([`Queue`]).
///
/// The [`push`](Self::push) method returns `Ok(usize)` — the number of
/// older frames evicted (always `0` for reliable channels). Use
/// [`push_checked`](Self::push_checked) for a richer [`PushOutcome`].
pub enum ChannelBuffer {
    /// Lossy mode: oldest frames are silently dropped when the byte budget is
    /// exceeded.
    Lossy(RingBuffer),
    /// Reliable mode: pushes are rejected with
    /// [`Error::ChannelFull`](crate::Error::ChannelFull) when the byte budget
    /// is exceeded.
    Reliable(Queue),
}

impl ChannelBuffer {
    /// Push a frame into the channel.
    ///
    /// For lossy channels, returns `Ok(usize)` — the number of older frames
    /// evicted to make room (or `0` if the frame was too large and silently
    /// discarded). For reliable channels, returns `Ok(0)` on success or
    /// [`Err(Error::ChannelFull)`](crate::Error::ChannelFull) if the byte
    /// budget would be exceeded.
    pub fn push(&self, frame: &[u8]) -> Result<usize, Error> {
        match self {
            Self::Lossy(rb) => Ok(rb.push(frame)),
            Self::Reliable(q) => q.push(frame).map(|()| 0),
        }
    }

    /// Push a frame with a richer outcome report.
    ///
    /// Like [`push`](Self::push), but returns [`PushOutcome`] for lossy
    /// channels, distinguishing between accepted frames and frames that were
    /// too large to ever fit.
    pub fn push_checked(&self, frame: &[u8]) -> Result<PushOutcome, Error> {
        match self {
            Self::Lossy(rb) => Ok(rb.push_checked(frame)),
            Self::Reliable(q) => q.push(frame).map(|()| PushOutcome::Accepted(0)),
        }
    }

    /// Drain all buffered frames into a single binary blob and clear the
    /// buffer.
    ///
    /// The wire format is identical for both variants — see
    /// [`RingBuffer::drain_all`] for details.
    #[must_use]
    pub fn drain_all(&self) -> Vec<u8> {
        match self {
            Self::Lossy(rb) => rb.drain_all(),
            Self::Reliable(q) => q.drain_all(),
        }
    }

    /// Read one frame from the front of the buffer (FIFO).
    ///
    /// Returns `None` if the buffer is empty.
    #[must_use]
    pub fn try_pop(&self) -> Option<Vec<u8>> {
        match self {
            Self::Lossy(rb) => rb.try_pop(),
            Self::Reliable(q) => q.try_pop(),
        }
    }

    /// Number of frames currently buffered.
    #[must_use]
    pub fn frame_count(&self) -> usize {
        match self {
            Self::Lossy(rb) => rb.frame_count(),
            Self::Reliable(q) => q.frame_count(),
        }
    }

    /// Number of bytes currently used (including per-frame length prefixes).
    #[must_use]
    pub fn bytes_used(&self) -> usize {
        match self {
            Self::Lossy(rb) => rb.bytes_used(),
            Self::Reliable(q) => q.bytes_used(),
        }
    }

    /// Clear all buffered frames.
    pub fn clear(&self) {
        match self {
            Self::Lossy(rb) => rb.clear(),
            Self::Reliable(q) => q.clear(),
        }
    }

    /// Returns `true` if this channel uses reliable (guaranteed-delivery)
    /// buffering.
    #[must_use]
    pub fn is_ordered(&self) -> bool {
        matches!(self, Self::Reliable(_))
    }
}

impl std::fmt::Debug for ChannelBuffer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Lossy(rb) => f.debug_tuple("ChannelBuffer::Lossy").field(rb).finish(),
            Self::Reliable(q) => f.debug_tuple("ChannelBuffer::Reliable").field(q).finish(),
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lossy_delegates_correctly() {
        let cb = ChannelBuffer::Lossy(RingBuffer::new(1024));

        let dropped = cb.push(b"alpha").unwrap();
        assert_eq!(dropped, 0);

        cb.push(b"beta").unwrap();
        assert_eq!(cb.frame_count(), 2);
        assert_eq!(cb.try_pop().unwrap(), b"alpha");
        assert_eq!(cb.try_pop().unwrap(), b"beta");
        assert!(cb.try_pop().is_none());
        assert!(!cb.is_ordered());
    }

    #[test]
    fn reliable_delegates_correctly() {
        let cb = ChannelBuffer::Reliable(Queue::new(1024));

        let dropped = cb.push(b"alpha").unwrap();
        assert_eq!(dropped, 0);

        cb.push(b"beta").unwrap();
        assert_eq!(cb.frame_count(), 2);
        assert_eq!(cb.try_pop().unwrap(), b"alpha");
        assert_eq!(cb.try_pop().unwrap(), b"beta");
        assert!(cb.try_pop().is_none());
        assert!(cb.is_ordered());
    }

    #[test]
    fn push_lossy_never_errors() {
        // Capacity for exactly 1 frame of 4 bytes (cost = 8).
        let cb = ChannelBuffer::Lossy(RingBuffer::new(8));

        // First push fills the buffer.
        assert_eq!(cb.push(b"aaaa").unwrap(), 0);

        // Second push evicts the first — but never errors.
        assert_eq!(cb.push(b"bbbb").unwrap(), 1);

        assert_eq!(cb.frame_count(), 1);
        assert_eq!(cb.try_pop().unwrap(), b"bbbb");
    }

    #[test]
    fn push_reliable_errors_when_full() {
        // Capacity for exactly 2 frames of 4 bytes (cost = 16).
        let cb = ChannelBuffer::Reliable(Queue::new(16));

        cb.push(b"aaaa").unwrap(); // cost 8
        cb.push(b"bbbb").unwrap(); // cost 8, total 16

        // Third push should fail.
        let err = cb.push(b"cccc").unwrap_err();
        assert!(matches!(err, Error::ChannelFull));

        // Original frames still intact.
        assert_eq!(cb.frame_count(), 2);
    }

    #[test]
    fn push_checked_lossy() {
        let cb = ChannelBuffer::Lossy(RingBuffer::new(8));
        assert_eq!(cb.push_checked(b"aaaa").unwrap(), PushOutcome::Accepted(0));
        assert_eq!(cb.push_checked(b"bbbb").unwrap(), PushOutcome::Accepted(1));

        // Too large for buffer (cost 104 > 8).
        assert_eq!(cb.push_checked(&[0u8; 100]).unwrap(), PushOutcome::TooLarge);
    }

    #[test]
    fn push_checked_reliable() {
        let cb = ChannelBuffer::Reliable(Queue::new(16));
        assert_eq!(cb.push_checked(b"aaaa").unwrap(), PushOutcome::Accepted(0));
        assert_eq!(cb.push_checked(b"bbbb").unwrap(), PushOutcome::Accepted(0));
        let err = cb.push_checked(b"cccc").unwrap_err();
        assert!(matches!(err, Error::ChannelFull));
    }

    #[test]
    fn drain_format_identical() {
        let lossy = ChannelBuffer::Lossy(RingBuffer::new(1024));
        let reliable = ChannelBuffer::Reliable(Queue::new(1024));

        // Push identical data to both.
        let frames: &[&[u8]] = &[b"hello", b"world", b"test"];
        for frame in frames {
            lossy.push(frame).unwrap();
            reliable.push(frame).unwrap();
        }

        let lossy_blob = lossy.drain_all();
        let reliable_blob = reliable.drain_all();

        assert_eq!(lossy_blob, reliable_blob);
        assert!(!lossy_blob.is_empty());
    }

    #[test]
    fn clear_delegates() {
        let cb = ChannelBuffer::Reliable(Queue::new(1024));
        cb.push(b"one").unwrap();
        cb.push(b"two").unwrap();
        assert_eq!(cb.frame_count(), 2);

        cb.clear();
        assert_eq!(cb.frame_count(), 0);
        assert_eq!(cb.bytes_used(), 0);
    }

    #[test]
    fn bytes_used_delegates() {
        let cb = ChannelBuffer::Lossy(RingBuffer::new(1024));
        assert_eq!(cb.bytes_used(), 0);

        cb.push(b"abc").unwrap(); // cost = 4 + 3 = 7
        assert_eq!(cb.bytes_used(), 7);
    }
}
