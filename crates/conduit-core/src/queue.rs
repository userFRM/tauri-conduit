//! Bounded frame queue with guaranteed delivery.
//!
//! [`Queue`] is the counterpart to [`crate::RingBuffer`]: instead of silently
//! dropping the oldest frames when the byte budget is exceeded, it rejects new
//! pushes with [`crate::Error::ChannelFull`]. This makes it suitable for
//! channels where every frame must be delivered (e.g. control messages,
//! transaction logs).
//!
//! # Wire format (`drain_all`)
//!
//! The wire format is identical to [`crate::RingBuffer::drain_all`]:
//!
//! ```text
//! [u32 LE frame_count]
//! [u32 LE len_1][bytes_1]
//! [u32 LE len_2][bytes_2]
//! ...
//! ```

use std::collections::VecDeque;
use std::sync::Mutex;

use crate::Error;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Per-frame overhead: 4 bytes for the u32 LE length prefix (matches
/// [`crate::ringbuf`]).
const FRAME_OVERHEAD: usize = 4;

// ---------------------------------------------------------------------------
// Inner
// ---------------------------------------------------------------------------

/// The unsynchronized interior of the queue.
struct QueueInner {
    /// Buffered frames in FIFO order.
    frames: VecDeque<Vec<u8>>,
    /// Total bytes used: sum of (FRAME_OVERHEAD + frame.len()) for each frame.
    bytes_used: usize,
    /// Maximum byte budget. `0` means unbounded.
    max_bytes: usize,
}

impl QueueInner {
    /// Create an empty inner buffer with the given byte budget.
    fn new(max_bytes: usize) -> Self {
        Self {
            frames: VecDeque::new(),
            bytes_used: 0,
            max_bytes,
        }
    }

    /// Cost of storing a single frame (length prefix + payload).
    #[inline]
    fn frame_cost(frame: &[u8]) -> usize {
        FRAME_OVERHEAD + frame.len()
    }
}

// ---------------------------------------------------------------------------
// Queue
// ---------------------------------------------------------------------------

/// Thread-safe, bounded FIFO queue with backpressure.
///
/// Unlike [`crate::RingBuffer`], this queue never drops frames. When the byte
/// budget would be exceeded, [`push`](Self::push) returns
/// [`Error::ChannelFull`](crate::Error::ChannelFull) and the frame is rejected.
/// A `max_bytes` of `0` means the queue is unbounded.
///
/// # Thread safety
///
/// All public methods take `&self` and synchronize via an internal [`Mutex`].
pub struct Queue {
    inner: Mutex<QueueInner>,
}

impl Queue {
    /// Create a queue with the given byte limit.
    ///
    /// A `max_bytes` of `0` means unbounded — pushes will never fail due to
    /// capacity.
    pub fn new(max_bytes: usize) -> Self {
        Self {
            inner: Mutex::new(QueueInner::new(max_bytes)),
        }
    }

    /// Create an unbounded queue.
    ///
    /// Equivalent to `Queue::new(0)`.
    pub fn unbounded() -> Self {
        Self::new(0)
    }

    /// Push a frame into the queue.
    ///
    /// Returns `Ok(())` if the frame was accepted. Returns
    /// [`Err(Error::ChannelFull)`](crate::Error::ChannelFull) if the frame
    /// (plus its 4-byte length prefix) would exceed `max_bytes`. When
    /// `max_bytes` is `0` (unbounded), pushes always succeed.
    pub fn push(&self, frame: &[u8]) -> Result<(), Error> {
        let cost = QueueInner::frame_cost(frame);
        let mut inner = self.inner.lock().expect("queue lock poisoned");

        if inner.max_bytes > 0 && inner.bytes_used + cost > inner.max_bytes {
            return Err(Error::ChannelFull);
        }

        inner.frames.push_back(frame.to_vec());
        inner.bytes_used += cost;
        Ok(())
    }

    /// Read one frame from the front of the queue (FIFO).
    ///
    /// Returns `None` if the queue is empty.
    #[must_use]
    pub fn try_pop(&self) -> Option<Vec<u8>> {
        let mut inner = self.inner.lock().expect("queue lock poisoned");
        let frame = inner.frames.pop_front()?;
        inner.bytes_used -= QueueInner::frame_cost(&frame);
        Some(frame)
    }

    /// Drain all queued frames into a single binary blob and clear the queue.
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
    /// Returns an empty `Vec` if the queue is empty.
    #[must_use]
    pub fn drain_all(&self) -> Vec<u8> {
        let mut inner = self.inner.lock().expect("queue lock poisoned");

        if inner.frames.is_empty() {
            return Vec::new();
        }

        // Pre-calculate output size: 4 (count) + sum of (4 + len) per frame.
        let output_size = 4 + inner.bytes_used;
        let mut buf = Vec::with_capacity(output_size);

        // Frame count header.
        let count = inner.frames.len() as u32;
        buf.extend_from_slice(&count.to_le_bytes());

        // Each frame: [u32 LE len][bytes].
        for frame in &inner.frames {
            let len = frame.len() as u32;
            buf.extend_from_slice(&len.to_le_bytes());
            buf.extend_from_slice(frame);
        }

        // Reset.
        inner.frames.clear();
        inner.bytes_used = 0;

        buf
    }

    /// Number of frames currently queued.
    #[must_use]
    pub fn frame_count(&self) -> usize {
        self.inner.lock().expect("queue lock poisoned").frames.len()
    }

    /// Number of bytes currently used (including per-frame length prefixes).
    #[must_use]
    pub fn bytes_used(&self) -> usize {
        self.inner.lock().expect("queue lock poisoned").bytes_used
    }

    /// Maximum byte budget (`0` means unbounded).
    #[must_use]
    pub fn max_bytes(&self) -> usize {
        self.inner.lock().expect("queue lock poisoned").max_bytes
    }

    /// Clear all queued frames.
    pub fn clear(&self) {
        let mut inner = self.inner.lock().expect("queue lock poisoned");
        inner.frames.clear();
        inner.bytes_used = 0;
    }
}

impl std::fmt::Debug for Queue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let inner = self.inner.lock().expect("queue lock poisoned");
        f.debug_struct("Queue")
            .field("frame_count", &inner.frames.len())
            .field("bytes_used", &inner.bytes_used)
            .field("max_bytes", &inner.max_bytes)
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
        let q = Queue::new(1024);
        q.push(b"alpha").unwrap();
        q.push(b"beta").unwrap();
        q.push(b"gamma").unwrap();

        assert_eq!(q.frame_count(), 3);
        assert_eq!(q.try_pop().unwrap(), b"alpha");
        assert_eq!(q.try_pop().unwrap(), b"beta");
        assert_eq!(q.try_pop().unwrap(), b"gamma");
        assert!(q.try_pop().is_none());
    }

    #[test]
    fn push_within_limit() {
        // Frame cost = 4 (overhead) + 4 (payload) = 8 bytes.
        // Two frames = 16 bytes, capacity = 16.
        let q = Queue::new(16);
        q.push(b"aaaa").unwrap(); // cost 8, total 8
        q.push(b"bbbb").unwrap(); // cost 8, total 16
        assert_eq!(q.frame_count(), 2);
        assert_eq!(q.bytes_used(), 16);
    }

    #[test]
    fn push_exceeds_limit() {
        // Capacity for exactly 2 frames of 4 bytes.
        let q = Queue::new(16);
        q.push(b"aaaa").unwrap(); // cost 8
        q.push(b"bbbb").unwrap(); // cost 8, total 16

        // Third push should fail.
        let err = q.push(b"cccc").unwrap_err();
        assert!(matches!(err, Error::ChannelFull));
        assert_eq!(err.to_string(), "channel full: byte limit reached");

        // Original frames are intact.
        assert_eq!(q.frame_count(), 2);
        assert_eq!(q.try_pop().unwrap(), b"aaaa");
        assert_eq!(q.try_pop().unwrap(), b"bbbb");
    }

    #[test]
    fn drain_all_format() {
        let q = Queue::new(1024);
        q.push(b"hello").unwrap();
        q.push(b"world").unwrap();

        let blob = q.drain_all();

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

        // Queue should be empty now.
        assert_eq!(q.frame_count(), 0);
        assert_eq!(q.bytes_used(), 0);
    }

    #[test]
    fn drain_frees_capacity() {
        let q = Queue::new(16);
        q.push(b"aaaa").unwrap(); // cost 8
        q.push(b"bbbb").unwrap(); // cost 8, total 16

        // Queue is full.
        assert!(q.push(b"cccc").is_err());

        // Drain frees everything.
        let blob = q.drain_all();
        assert!(!blob.is_empty());
        assert_eq!(q.bytes_used(), 0);

        // Now pushes succeed again.
        q.push(b"dddd").unwrap();
        q.push(b"eeee").unwrap();
        assert_eq!(q.frame_count(), 2);
    }

    #[test]
    fn unbounded_mode() {
        let q = Queue::unbounded();
        assert_eq!(q.max_bytes(), 0);

        // Push a large number of frames — should never fail.
        for i in 0u32..10_000 {
            q.push(&i.to_le_bytes()).unwrap();
        }
        assert_eq!(q.frame_count(), 10_000);
    }

    #[test]
    fn frame_count_and_bytes() {
        let q = Queue::new(1024);

        assert_eq!(q.frame_count(), 0);
        assert_eq!(q.bytes_used(), 0);
        assert_eq!(q.max_bytes(), 1024);

        q.push(b"abc").unwrap(); // cost = 4 + 3 = 7
        assert_eq!(q.frame_count(), 1);
        assert_eq!(q.bytes_used(), 7);

        q.push(b"de").unwrap(); // cost = 4 + 2 = 6
        assert_eq!(q.frame_count(), 2);
        assert_eq!(q.bytes_used(), 13);

        let _ = q.try_pop();
        assert_eq!(q.frame_count(), 1);
        assert_eq!(q.bytes_used(), 6);
    }

    #[test]
    fn clear() {
        let q = Queue::new(1024);
        q.push(b"one").unwrap();
        q.push(b"two").unwrap();
        q.push(b"three").unwrap();

        assert_eq!(q.frame_count(), 3);
        q.clear();
        assert_eq!(q.frame_count(), 0);
        assert_eq!(q.bytes_used(), 0);
        assert!(q.try_pop().is_none());
    }

    #[test]
    fn concurrent_push_pop() {
        use std::sync::Arc;

        let q = Arc::new(Queue::unbounded());
        let q_producer = Arc::clone(&q);
        let q_consumer = Arc::clone(&q);

        let producer = std::thread::spawn(move || {
            for i in 0u32..1000 {
                q_producer.push(&i.to_le_bytes()).unwrap();
            }
        });

        let consumer = std::thread::spawn(move || {
            let mut popped = 0usize;
            loop {
                if q_consumer.try_pop().is_some() {
                    popped += 1;
                }
                if popped >= 1000 {
                    break;
                }
                // Yield to let the producer make progress.
                std::thread::yield_now();
            }
            popped
        });

        producer.join().unwrap();
        let consumer_popped = consumer.join().unwrap();

        // Between the consumer and any remaining frames, we should account for
        // all 1000 pushes.
        let remaining = q.frame_count();
        assert_eq!(consumer_popped + remaining, 1000);
    }

    #[test]
    fn empty_drain() {
        let q = Queue::new(1024);
        let blob = q.drain_all();
        assert!(blob.is_empty());
    }

    #[test]
    fn drain_then_push() {
        let q = Queue::new(1024);
        q.push(b"first").unwrap();
        let blob = q.drain_all();
        assert!(!blob.is_empty());

        // Queue is empty after drain; push more.
        q.push(b"second").unwrap();
        assert_eq!(q.frame_count(), 1);
        assert_eq!(q.try_pop().unwrap(), b"second");
    }
}
