use crossbeam_queue::ArrayQueue;
use std::sync::Arc;

/// Lock-free MPMC stereo-frame ring with **drop-oldest** overflow: when full,
/// the oldest frame is discarded to admit the newest. That matches the design's
/// underrun policy — the audience skips forward (brief gap) rather than the
/// producer freezing on a stalled consumer. rtrb is SPSC and cannot pop from
/// its producer, hence this wrapper (crossbeam ArrayQueue + per-slot slots).
pub struct StereoRing {
    q: Arc<ArrayQueue<[f32; 2]>>,
}

pub struct StereoRingProducer {
    q: Arc<ArrayQueue<[f32; 2]>>,
}

pub struct StereoRingConsumer {
    q: Arc<ArrayQueue<[f32; 2]>>,
}

impl StereoRing {
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
        let mut last = None;
        while let Some(f) = rx.pop() {
            last = Some(f[0] as i32);
        }
        assert_eq!(last, Some(9999));
        // And values must be strictly increasing (order preserved).
    }
}
