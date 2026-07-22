use rtrb::{Consumer, Producer, RingBuffer};

/// Lock-free SPSC stereo-frame ring. Decouples the realtime audio callback
/// (producer) from the encoder/network consumer.
pub struct StereoRing {
    tx: Producer<[f32; 2]>,
    rx: Consumer<[f32; 2]>,
}

impl StereoRing {
    pub fn new(frames: usize) -> Self {
        let (tx, rx) = RingBuffer::<[f32; 2]>::new(frames);
        Self { tx, rx }
    }
    /// Push a frame; if full, drop the oldest first (broadcast resilience —
    /// audience hears a brief skip, DJ/monitor unaffected).
    pub fn push(&mut self, frame: [f32; 2]) {
        while self.tx.push(frame).is_err() {
            if self.rx.pop().is_err() {
                break; // make room failed (shouldn't happen); bail
            }
        }
    }
    pub fn pop(&mut self) -> Option<[f32; 2]> {
        self.rx.pop().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn burst_beyond_capacity_drops_oldest_keeps_order() {
        let mut r = StereoRing::new(4); // 4 frames
        // push 6 frames; capacity 4 → frames 0,1 dropped, 2..5 remain in order
        for i in 0..6i32 {
            r.push([i as f32, i as f32]);
        }
        let mut got = Vec::new();
        while let Some(f) = r.pop() {
            got.push(f[0] as i32);
        }
        assert_eq!(got, vec![2, 3, 4, 5]);
    }
}
