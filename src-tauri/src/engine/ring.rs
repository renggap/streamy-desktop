use crossbeam_queue::ArrayQueue;
use std::sync::Arc;

/// Lock-free MPMC stereo-frame ring with **drop-oldest** overflow: when full,
/// the oldest frame is discarded to admit the newest. That matches the design's
/// underrun policy — the audience skips forward (brief gap) rather than the
/// producer freezing on a stalled consumer. rtrb is SPSC and cannot pop from
/// its producer, hence this wrapper (crossbeam ArrayQueue + per-slot slots).
///
/// Handles are cheap `Arc` clones — both sides can be shared (e.g. the mic
/// callback and the mic thread).
#[derive(Clone)]
pub struct StereoRingProducer {
    q: Arc<ArrayQueue<[f32; 2]>>,
}

#[derive(Clone)]
pub struct StereoRingConsumer {
    q: Arc<ArrayQueue<[f32; 2]>>,
}

/// Constructor namespace: `StereoRing::new(cap) -> (producer, consumer)`.
pub struct StereoRing;

impl StereoRing {
    /// Named `new` for call-site ergonomics; returns a (producer, consumer)
    /// pair rather than `Self` by design.
    #[allow(clippy::new_without_default, clippy::new_ret_no_self)]
    pub fn new(frames: usize) -> (StereoRingProducer, StereoRingConsumer) {
        let q = Arc::new(ArrayQueue::new(frames.max(1)));
        (
            StereoRingProducer { q: q.clone() },
            StereoRingConsumer { q },
        )
    }
}

impl StereoRingProducer {
    /// Push a frame; if full, the OLDEST frame is evicted to admit it
    /// (`ArrayQueue::force_push` swaps the tail slot in place — no retry dance
    /// needed). Broadcast resilience: the audience skips forward, not freezes.
    pub fn push(&self, frame: [f32; 2]) {
        let _ = self.q.force_push(frame);
    }
}

impl StereoRingConsumer {
    pub fn pop(&self) -> Option<[f32; 2]> {
        self.q.pop()
    }

    /// Frames currently queued (diagnostics/tests).
    pub fn len(&self) -> usize {
        self.q.len()
    }

    /// Frames queued == 0.
    pub fn is_empty(&self) -> bool {
        self.q.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn burst_beyond_capacity_drops_oldest_keeps_order() {
        let (tx, rx) = StereoRing::new(4);
        for i in 0..6i32 {
            tx.push([i as f32, i as f32]);
        }
        let mut got = Vec::new();
        while let Some(f) = rx.pop() {
            got.push(f[0] as i32);
        }
        assert_eq!(got, vec![2, 3, 4, 5]);
    }

    #[test]
    fn spsc_no_loss_under_capacity() {
        let (tx, rx) = StereoRing::new(1024);
        for i in 0..1000i32 {
            tx.push([i as f32, -(i as f32)]);
        }
        for i in 0..1000i32 {
            let f = rx.pop().expect("frame present");
            assert_eq!(f[0] as i32, i);
            assert_eq!(f[1] as i32, -i);
        }
        assert!(rx.pop().is_none());
    }

    #[test]
    fn stress_producer_faster_than_consumer_drops_oldest_not_newest() {
        // Producer pushes 10k; consumer drains lazily. Whatever remains must be
        // the NEWEST contiguous tail (drop-oldest), ending at 9999.
        let (tx, rx) = StereoRing::new(64);
        for i in 0..10_000i32 {
            tx.push([i as f32, i as f32]);
            if i % 7 == 0 {
                let _ = rx.pop();
            }
        }
        // Drain the remainder; the last value must be the newest frame (9999),
        // proving the newest data survived (vs rtrb push which drops newest).
        let mut drained = Vec::new();
        while let Some(f) = rx.pop() {
            drained.push(f[0] as i32);
        }
        assert_eq!(drained.last(), Some(&9999));
        assert!(
            drained.windows(2).all(|w| w[0] < w[1]),
            "surviving frames reordered"
        );
    }
}

#[test]
fn mpmc_multiple_producers_order_is_fifo_per_slot() {
    // Two producers interleave pushes (no consumer during the run, so
    // capacity-many survive). Invariants: each producer's surviving
    // frames stay in push order (strictly increasing ids), no frame is
    // returned twice, and exactly capacity survive.
    let (tx, rx) = StereoRing::new(256);
    let t2 = tx.clone();
    let h = std::thread::spawn(move || {
        for i in 0..500 {
            t2.push([-1.0, i as f32]);
        }
    });
    for i in 0..500 {
        tx.push([1.0, i as f32]);
    }
    h.join().unwrap();
    let mut last_pos: Option<i32> = None;
    let mut last_neg: Option<i32> = None;
    let mut count = 0usize;
    while let Some(f) = rx.pop() {
        count += 1;
        let id = f[1] as i32;
        if f[0] > 0.0 {
            assert!(last_pos.map_or(true, |p| id > p), "pos producer reordered");
            last_pos = Some(id);
        } else {
            assert!(last_neg.map_or(true, |p| id > p), "neg producer reordered");
            last_neg = Some(id);
        }
    }
    assert_eq!(count, 256, "exactly capacity frames survive");
}

#[test]
fn zero_capacity_ring_still_accepts_and_yields() {
    // new(0) clamps to 1: force_push evicts, pop returns the survivor.
    let (tx, rx) = StereoRing::new(0);
    tx.push([1.0, 1.0]);
    tx.push([2.0, 2.0]);
    let f = rx.pop().expect("one frame survives");
    assert_eq!(f, [2.0, 2.0], "only newest survives in a 1-slot ring");
    assert!(rx.is_empty());
}
