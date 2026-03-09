//! Unified channel buffer that wraps either a lossy or ordered strategy.
//!
//! [`ChannelBuffer`] is an enum over [`crate::RingBuffer`] (lossy) and
//! [`crate::Queue`] (guaranteed delivery). It provides a single
//! API surface so that the plugin layer can treat all channels uniformly.

use crate::error::Error;
use crate::queue::Queue;
use crate::ringbuf::RingBuffer;

// ---------------------------------------------------------------------------
// ChannelBuffer
// ---------------------------------------------------------------------------

/// A channel buffer that is either lossy ([`RingBuffer`]) or ordered
/// ([`Queue`]).
///
/// The [`push`](Self::push) method returns `Ok(dropped_count)` — for ordered
/// buffers the count is always `0`; for lossy buffers it is the number of
/// oldest frames that were evicted to make room.
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
    /// Returns `Ok(n)` where `n` is the number of frames dropped to make room
    /// (always `0` for ordered channels). Returns
    /// [`Err(Error::ChannelFull)`](crate::Error::ChannelFull) if an ordered
    /// channel's byte budget would be exceeded.
    pub fn push(&self, frame: &[u8]) -> Result<usize, Error> {
        match self {
            Self::Lossy(rb) => Ok(rb.push(frame)),
            Self::Reliable(rb) => rb.push(frame).map(|()| 0),
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
            Self::Reliable(rb) => rb.drain_all(),
        }
    }

    /// Read one frame from the front of the buffer (FIFO).
    ///
    /// Returns `None` if the buffer is empty.
    #[must_use]
    pub fn try_pop(&self) -> Option<Vec<u8>> {
        match self {
            Self::Lossy(rb) => rb.try_pop(),
            Self::Reliable(rb) => rb.try_pop(),
        }
    }

    /// Number of frames currently buffered.
    #[must_use]
    pub fn frame_count(&self) -> usize {
        match self {
            Self::Lossy(rb) => rb.frame_count(),
            Self::Reliable(rb) => rb.frame_count(),
        }
    }

    /// Number of bytes currently used (including per-frame length prefixes).
    #[must_use]
    pub fn bytes_used(&self) -> usize {
        match self {
            Self::Lossy(rb) => rb.bytes_used(),
            Self::Reliable(rb) => rb.bytes_used(),
        }
    }

    /// Clear all buffered frames.
    pub fn clear(&self) {
        match self {
            Self::Lossy(rb) => rb.clear(),
            Self::Reliable(rb) => rb.clear(),
        }
    }

    /// Returns `true` if this channel uses ordered (guaranteed-delivery)
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
            Self::Reliable(rb) => f.debug_tuple("ChannelBuffer::Reliable").field(rb).finish(),
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
    fn ordered_delegates_correctly() {
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
        let dropped = cb.push(b"aaaa").unwrap();
        assert_eq!(dropped, 0);

        // Second push evicts the first — but never errors.
        let dropped = cb.push(b"bbbb").unwrap();
        assert_eq!(dropped, 1);

        assert_eq!(cb.frame_count(), 1);
        assert_eq!(cb.try_pop().unwrap(), b"bbbb");
    }

    #[test]
    fn push_ordered_errors_when_full() {
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
    fn drain_format_identical() {
        let lossy = ChannelBuffer::Lossy(RingBuffer::new(1024));
        let ordered = ChannelBuffer::Reliable(Queue::new(1024));

        // Push identical data to both.
        let frames: &[&[u8]] = &[b"hello", b"world", b"test"];
        for frame in frames {
            lossy.push(frame).unwrap();
            ordered.push(frame).unwrap();
        }

        let lossy_blob = lossy.drain_all();
        let ordered_blob = ordered.drain_all();

        assert_eq!(lossy_blob, ordered_blob);
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
