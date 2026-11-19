//! Lock-free single-producer / single-consumer ring buffer for f32 samples.
//! Used to move audio between the capture thread, the playback thread and
//! the analyzer without ever locking inside an audio callback.

use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

pub struct Ring {
    buf: Box<[UnsafeCell<f32>]>,
    mask: usize,
    head: AtomicUsize, // next write index (producer)
    tail: AtomicUsize, // next read index (consumer)
}

// Safety: exactly one producer thread writes slots in [tail+len, ...) and one
// consumer thread reads slots in [tail, head). Indices are published with
// Release/Acquire ordering so a slot is never read and written at once.
unsafe impl Send for Ring {}
unsafe impl Sync for Ring {}

impl Ring {
    pub fn new(capacity: usize) -> Arc<Self> {
        let cap = capacity.max(2).next_power_of_two();
        let buf = (0..cap).map(|_| UnsafeCell::new(0.0)).collect::<Vec<_>>();
        Arc::new(Self {
            buf: buf.into_boxed_slice(),
            mask: cap - 1,
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
        })
    }

    #[inline]
    pub fn capacity(&self) -> usize {
        self.mask + 1
    }

    /// Number of samples waiting to be read.
    #[inline]
    pub fn len(&self) -> usize {
        self.head
            .load(Ordering::Acquire)
            .wrapping_sub(self.tail.load(Ordering::Acquire))
    }

    /// Free slots available to the producer.
    #[inline]
    pub fn free(&self) -> usize {
        self.capacity() - self.len()
    }

    /// Producer only. Returns false when full (sample dropped).
    #[inline]
    pub fn push(&self, v: f32) -> bool {
        let h = self.head.load(Ordering::Relaxed);
        let t = self.tail.load(Ordering::Acquire);
        if h.wrapping_sub(t) >= self.capacity() {
            return false;
        }
        unsafe { *self.buf[h & self.mask].get() = v };
        self.head.store(h.wrapping_add(1), Ordering::Release);
        true
    }

    /// Consumer only.
    #[inline]
    pub fn pop(&self) -> Option<f32> {
        let t = self.tail.load(Ordering::Relaxed);
        let h = self.head.load(Ordering::Acquire);
        if h == t {
            return None;
        }
        let v = unsafe { *self.buf[t & self.mask].get() };
        self.tail.store(t.wrapping_add(1), Ordering::Release);
        Some(v)
    }

    /// Consumer only: discard up to `n` samples (used for drift correction).
    pub fn skip(&self, n: usize) {
        let t = self.tail.load(Ordering::Relaxed);
        let n = n.min(self.len());
        self.tail.store(t.wrapping_add(n), Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_pop_wrap() {
        let r = Ring::new(4);
        for round in 0..10 {
            for i in 0..4 {
                assert!(r.push((round * 4 + i) as f32));
            }
            assert!(!r.push(99.0), "should be full");
            for i in 0..4 {
                assert_eq!(r.pop(), Some((round * 4 + i) as f32));
            }
            assert_eq!(r.pop(), None);
        }
    }

    #[test]
    fn skip_and_len() {
        let r = Ring::new(8);
        for i in 0..6 {
            r.push(i as f32);
        }
        assert_eq!(r.len(), 6);
        r.skip(4);
        assert_eq!(r.len(), 2);
        assert_eq!(r.pop(), Some(4.0));
        r.skip(100);
        assert_eq!(r.len(), 0);
    }

    #[test]
    fn threaded_spsc() {
        let r = Ring::new(1024);
        let p = r.clone();
        let n = 200_000u32;
        let th = std::thread::spawn(move || {
            let mut i = 0u32;
            while i < n {
                if p.push(i as f32) {
                    i += 1;
                }
            }
        });
        let mut expect = 0u32;
        while expect < n {
            if let Some(v) = r.pop() {
                assert_eq!(v, expect as f32);
                expect += 1;
            }
        }
        th.join().unwrap();
    }
}
