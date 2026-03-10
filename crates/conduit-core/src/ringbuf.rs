//! In-process ring buffer for high-frequency streaming.
//!
//! [`RingBuffer`] is the breakthrough component of tauri-conduit: an
//! in-process circular buffer that lets the Rust backend stream binary frames
//! to the WebView frontend without serialization, IPC, or inter-process shared
//! memory. The custom protocol handler (`conduit://`) reads directly from it.
//!
//! # Design
//!
//! The buffer stores variable-length frames with a configurable byte budget
//! (default 64 KB). When the budget is exceeded, the oldest frames are dropped
//! to make room — this is lossy by design, because the JS consumer is expected
//! to drain fast enough for real-time use cases (market data, sensor telemetry,
//! audio buffers).
//!
//! # Wire format (`drain_all`)
//!
//! ```text
//! [u32 LE frame_count]
//! [u32 LE len_1][bytes_1]
//! [u32 LE len_2][bytes_2]
//! ...
//! ```

use std::collections::VecDeque;
use std::sync::Mutex;

use crate::codec::DRAIN_FRAME_OVERHEAD;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Default capacity in bytes (64 KB).
const DEFAULT_CAPACITY: usize = 64 * 1024;

// ---------------------------------------------------------------------------
// Inner
// ---------------------------------------------------------------------------

/// The unsynchronized interior of the ring buffer.
struct Inner {
    /// Buffered frames in FIFO order.
    frames: VecDeque<Vec<u8>>,
    /// Total bytes used: sum of (DRAIN_FRAME_OVERHEAD + frame.len()) for each frame.
    bytes_used: usize,
    /// Maximum byte budget.
    capacity: usize,
}

impl Inner {
    /// Create an empty inner buffer with the given byte budget.
    fn new(capacity: usize) -> Self {
        Self {
            frames: VecDeque::new(),
            bytes_used: 0,
            capacity,
        }
    }

    /// Cost of storing a single frame (length prefix + payload).
    #[inline]
    fn frame_cost(frame: &[u8]) -> usize {
        DRAIN_FRAME_OVERHEAD + frame.len()
    }

    /// Drop the oldest frame, adjusting the byte counter. Returns `true` if
    /// a frame was actually removed.
    fn drop_oldest(&mut self) -> bool {
        if let Some(old) = self.frames.pop_front() {
            self.bytes_used -= Self::frame_cost(&old);
            true
        } else {
            false
        }
    }
}

// ---------------------------------------------------------------------------
// PushOutcome
// ---------------------------------------------------------------------------

/// Outcome of a [`RingBuffer::push`] operation.
///
/// Distinguishes between a frame being accepted (possibly with evictions)
/// and a frame being discarded because it can never fit in the buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PushOutcome {
    /// Frame was accepted. The `usize` is the number of older frames
    /// that were evicted to make room (may be `0`).
    Accepted(usize),
    /// Frame was too large to ever fit in this buffer (even when empty)
    /// and was silently discarded. No data was written.
    TooLarge,
}

// ---------------------------------------------------------------------------
// RingBuffer
// ---------------------------------------------------------------------------

/// Thread-safe, in-process circular buffer for streaming binary frames.
///
/// Frames are variable-length byte slices stored with a u32 LE length prefix.
/// The buffer enforces a byte budget; when a push would exceed the budget the
/// oldest frames are silently dropped (lossy back-pressure).
///
/// # Thread safety
///
/// All public methods take `&self` and synchronize via an internal [`Mutex`].
/// Contention is expected to be low: typically one producer thread and one
/// consumer (the custom protocol handler draining on a `fetch` call).
pub struct RingBuffer {
    inner: Mutex<Inner>,
}

impl RingBuffer {
    /// Create a ring buffer with the given byte capacity.
    ///
    /// # Panics
    ///
    /// Panics if `capacity` is less than `DRAIN_FRAME_OVERHEAD + 1` (5 bytes),
    /// since at least a 1-byte frame must be storable.
    pub fn new(capacity: usize) -> Self {
        assert!(
            capacity > DRAIN_FRAME_OVERHEAD,
            "capacity must be at least {} bytes (DRAIN_FRAME_OVERHEAD + 1)",
            DRAIN_FRAME_OVERHEAD + 1,
        );
        Self {
            inner: Mutex::new(Inner::new(capacity)),
        }
    }

    /// Create a ring buffer with the default capacity (64 KB).
    pub fn with_default_capacity() -> Self {
        Self::new(DEFAULT_CAPACITY)
    }

    /// Push a frame into the buffer.
    ///
    /// If the frame (plus its 4-byte length prefix) would exceed the byte
    /// budget, the oldest frames are dropped until there is room. Returns the
    /// number of frames that were dropped to make space.
    ///
    /// If the frame itself is larger than the total capacity it is silently
    /// discarded and the return value is `0`.
    pub fn push(&self, frame: &[u8]) -> usize {
        match self.push_checked(frame) {
            PushOutcome::Accepted(n) => n,
            PushOutcome::TooLarge => 0,
        }
    }

    /// Push a frame with a richer outcome report.
    ///
    /// Like [`push`](Self::push), but returns [`PushOutcome::TooLarge`] when
    /// the frame can never fit, instead of silently returning `0`.
    #[must_use]
    pub fn push_checked(&self, frame: &[u8]) -> PushOutcome {
        let cost = Inner::frame_cost(frame);
        let mut inner = crate::lock_or_recover(&self.inner);

        // Frame too large for this buffer — discard it.
        if cost > inner.capacity {
            return PushOutcome::TooLarge;
        }

        let mut dropped = 0usize;
        while inner.bytes_used + cost > inner.capacity {
            if !inner.drop_oldest() {
                break;
            }
            dropped += 1;
        }

        inner.frames.push_back(frame.to_vec());
        inner.bytes_used += cost;
        PushOutcome::Accepted(dropped)
    }

    /// Drain all buffered frames into a single binary blob and clear the
    /// buffer.
    ///
    /// # Wire format
    ///
    /// ```text
    /// [u32 LE frame_count]
    /// [u32 LE len_1][bytes_1]
    /// [u32 LE len_2][bytes_2]
    /// ...
    /// ```
    ///
    /// Returns an empty `Vec` if the buffer is empty.
    #[must_use]
    pub fn drain_all(&self) -> Vec<u8> {
        // Swap the frames out under the lock, then serialize without contention.
        let (mut frames, bytes_used) = {
            let mut inner = crate::lock_or_recover(&self.inner);
            if inner.frames.is_empty() {
                return Vec::new();
            }
            let frames = std::mem::take(&mut inner.frames);
            let bytes_used = inner.bytes_used;
            inner.bytes_used = 0;
            (frames, bytes_used)
        };
        // Lock released — serialize without holding the mutex.
        let output_size = 4usize.saturating_add(bytes_used);
        let mut buf = Vec::with_capacity(output_size);

        // Frame count header.
        let count = frames.len() as u32;
        buf.extend_from_slice(&count.to_le_bytes());

        // Each frame: [u32 LE len][bytes].
        for frame in frames.make_contiguous() {
            let len = frame.len() as u32;
            buf.extend_from_slice(&len.to_le_bytes());
            buf.extend_from_slice(frame);
        }

        buf
    }

    /// Read one frame from the front of the buffer (FIFO).
    ///
    /// Returns `None` if the buffer is empty.
    #[must_use]
    pub fn try_pop(&self) -> Option<Vec<u8>> {
        let mut inner = crate::lock_or_recover(&self.inner);
        let frame = inner.frames.pop_front()?;
        inner.bytes_used -= Inner::frame_cost(&frame);
        Some(frame)
    }

    /// Number of frames currently buffered.
    #[must_use]
    pub fn frame_count(&self) -> usize {
        crate::lock_or_recover(&self.inner).frames.len()
    }

    /// Number of bytes currently used (including per-frame length prefixes).
    #[must_use]
    pub fn bytes_used(&self) -> usize {
        crate::lock_or_recover(&self.inner).bytes_used
    }

    /// Total byte capacity of the buffer.
    #[must_use]
    pub fn capacity(&self) -> usize {
        crate::lock_or_recover(&self.inner).capacity
    }

    /// Clear all buffered frames.
    pub fn clear(&self) {
        let mut inner = crate::lock_or_recover(&self.inner);
        inner.frames.clear();
        inner.bytes_used = 0;
    }
}

impl std::fmt::Debug for RingBuffer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let inner = crate::lock_or_recover(&self.inner);
        f.debug_struct("RingBuffer")
            .field("frame_count", &inner.frames.len())
            .field("bytes_used", &inner.bytes_used)
            .field("capacity", &inner.capacity)
            .finish()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_and_pop() {
        let rb = RingBuffer::new(1024);
        let _ = rb.push(b"alpha");
        let _ = rb.push(b"beta");
        let _ = rb.push(b"gamma");

        assert_eq!(rb.frame_count(), 3);
        assert_eq!(rb.try_pop().unwrap(), b"alpha");
        assert_eq!(rb.try_pop().unwrap(), b"beta");
        assert_eq!(rb.try_pop().unwrap(), b"gamma");
        assert!(rb.try_pop().is_none());
    }

    #[test]
    fn drain_all_format() {
        let rb = RingBuffer::new(1024);
        let _ = rb.push(b"hello");
        let _ = rb.push(b"world");

        let blob = rb.drain_all();

        // Parse: [u32 count][u32 len][bytes]...
        let count = u32::from_le_bytes(blob[0..4].try_into().unwrap());
        assert_eq!(count, 2);

        let len1 = u32::from_le_bytes(blob[4..8].try_into().unwrap()) as usize;
        assert_eq!(len1, 5);
        assert_eq!(&blob[8..8 + len1], b"hello");

        let offset2 = 8 + len1;
        let len2 = u32::from_le_bytes(blob[offset2..offset2 + 4].try_into().unwrap()) as usize;
        assert_eq!(len2, 5);
        assert_eq!(&blob[offset2 + 4..offset2 + 4 + len2], b"world");

        // Buffer should be empty now.
        assert_eq!(rb.frame_count(), 0);
        assert_eq!(rb.bytes_used(), 0);
    }

    #[test]
    fn overflow_drops_oldest() {
        // Capacity for exactly 2 frames of 4 bytes each:
        //   frame cost = 4 (overhead) + 4 (payload) = 8 bytes
        //   2 frames = 16 bytes
        let rb = RingBuffer::new(16);

        let dropped = rb.push(b"aaaa"); // cost 8, total 8
        assert_eq!(dropped, 0);

        let dropped = rb.push(b"bbbb"); // cost 8, total 16
        assert_eq!(dropped, 0);

        // Third push must drop the oldest to fit.
        let dropped = rb.push(b"cccc"); // drops "aaaa"
        assert_eq!(dropped, 1);

        assert_eq!(rb.frame_count(), 2);
        assert_eq!(rb.try_pop().unwrap(), b"bbbb");
        assert_eq!(rb.try_pop().unwrap(), b"cccc");
    }

    #[test]
    fn empty_drain() {
        let rb = RingBuffer::new(1024);
        let blob = rb.drain_all();
        assert!(blob.is_empty());
    }

    #[test]
    fn frame_count_and_bytes() {
        let rb = RingBuffer::new(1024);

        assert_eq!(rb.frame_count(), 0);
        assert_eq!(rb.bytes_used(), 0);
        assert_eq!(rb.capacity(), 1024);

        let _ = rb.push(b"abc"); // cost = 4 + 3 = 7
        assert_eq!(rb.frame_count(), 1);
        assert_eq!(rb.bytes_used(), 7);

        let _ = rb.push(b"de"); // cost = 4 + 2 = 6
        assert_eq!(rb.frame_count(), 2);
        assert_eq!(rb.bytes_used(), 13);

        let _ = rb.try_pop();
        assert_eq!(rb.frame_count(), 1);
        assert_eq!(rb.bytes_used(), 6);
    }

    #[test]
    fn clear() {
        let rb = RingBuffer::new(1024);
        let _ = rb.push(b"one");
        let _ = rb.push(b"two");
        let _ = rb.push(b"three");

        assert_eq!(rb.frame_count(), 3);
        rb.clear();
        assert_eq!(rb.frame_count(), 0);
        assert_eq!(rb.bytes_used(), 0);
        assert!(rb.try_pop().is_none());
    }

    #[tokio::test]
    async fn concurrent_push_pop() {
        use std::sync::Arc;

        let rb = Arc::new(RingBuffer::new(64 * 1024));
        let rb_producer = Arc::clone(&rb);
        let rb_consumer = Arc::clone(&rb);

        let producer = tokio::spawn(async move {
            for i in 0u32..1000 {
                let _ = rb_producer.push(&i.to_le_bytes());
            }
        });

        let consumer = tokio::spawn(async move {
            let mut popped = 0usize;
            // Keep trying until the producer is done and the buffer is empty.
            loop {
                if let Some(_frame) = rb_consumer.try_pop() {
                    popped += 1;
                } else {
                    // Yield to let the producer make progress.
                    tokio::task::yield_now().await;
                }
                // Safety valve: once we know the producer pushed 1000, stop
                // when the buffer is empty.
                if popped >= 1000 {
                    break;
                }
            }
            popped
        });

        producer.await.unwrap();
        // Drain whatever the consumer missed.
        let consumer_popped = consumer.await.unwrap();

        // Between the consumer and any remaining frames, we should account for
        // all 1000 pushes (some may have been dropped due to timing, but with
        // 64 KB capacity and 8 bytes per frame, nothing should be lost here).
        let remaining = rb.frame_count();
        assert_eq!(consumer_popped + remaining, 1000);
    }

    #[test]
    fn single_large_frame() {
        // Buffer capacity is 32 bytes. A frame of 100 bytes costs 104 bytes
        // — larger than capacity. It should be silently discarded.
        let rb = RingBuffer::new(32);
        let _ = rb.push(b"ok"); // cost 6, fits
        let dropped = rb.push(&[0xFFu8; 100]); // cost 104, too large
        assert_eq!(dropped, 0); // not counted as "dropped oldest"

        // The small frame should still be there.
        assert_eq!(rb.frame_count(), 1);
        assert_eq!(rb.try_pop().unwrap(), b"ok");
    }

    #[test]
    fn drain_then_push() {
        let rb = RingBuffer::new(1024);
        let _ = rb.push(b"first");
        let blob = rb.drain_all();
        assert!(!blob.is_empty());

        // Buffer is empty after drain; push more.
        let _ = rb.push(b"second");
        assert_eq!(rb.frame_count(), 1);
        assert_eq!(rb.try_pop().unwrap(), b"second");
    }

    #[test]
    fn overflow_cascade() {
        // Capacity for exactly one 4-byte frame (cost = 8).
        let rb = RingBuffer::new(8);

        let _ = rb.push(b"aaaa"); // cost 8, fills completely
        assert_eq!(rb.frame_count(), 1);

        // Push a larger frame (6 bytes, cost 10 > 8) — too large for buffer.
        let dropped = rb.push(&[0u8; 6]);
        // The frame cannot fit even in an empty buffer, so it's discarded.
        assert_eq!(dropped, 0);

        // Original frame should still be intact.
        assert_eq!(rb.frame_count(), 1);
        assert_eq!(rb.try_pop().unwrap(), b"aaaa");
    }

    #[test]
    #[should_panic(expected = "capacity must be at least 5 bytes")]
    fn tiny_capacity_panics() {
        RingBuffer::new(4); // equal to DRAIN_FRAME_OVERHEAD, but less than DRAIN_FRAME_OVERHEAD + 1
    }

    #[test]
    fn with_default_capacity() {
        let rb = RingBuffer::with_default_capacity();
        assert_eq!(rb.capacity(), 64 * 1024);
    }
}
