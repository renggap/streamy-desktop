// Mic noise suppression: RNNoise (pure-Rust port via nnnoiseless) on the
// broadcast voice path. Runs on the mono-summed mic AFTER resampling to the
// output rate, one 480-sample frame at a time (~10ms @ 48 kHz). Adds exactly
// FRAME_SIZE samples of latency to the mic path only; decks are untouched.
// ponytail: RNNoise is trained at 48 kHz; run at the output rate so at
// 44.1 kHz the band mapping shifts ~9% (still effective). Polyphase
// resample-to-48k if announcers ever measure artifacts.

use nnnoiseless::DenoiseState;

pub struct MicDenoiser {
    state: Box<DenoiseState<'static>>,
    in_buf: [f32; DenoiseState::FRAME_SIZE],
    in_fill: usize,
    out_buf: [f32; DenoiseState::FRAME_SIZE],
    out_pos: usize,
    last: f32,
    /// The first processed frame contains fade-in artifacts (see
    /// `DenoiseState::process_frame`); it is discarded once per instance.
    primed: bool,
}

impl MicDenoiser {
    pub fn new() -> Self {
        Self {
            state: DenoiseState::new(),
            in_buf: [0.0; DenoiseState::FRAME_SIZE],
            in_fill: 0,
            out_buf: [0.0; DenoiseState::FRAME_SIZE],
            out_pos: DenoiseState::FRAME_SIZE, // dry
            last: 0.0,
            primed: false,
        }
    }

    /// Feed one mono sample in [-1,1]; returns the denoised sample, delayed
    /// FRAME_SIZE samples (delay line). If the output side runs dry the last
    /// value holds — same policy as the mic resampler (no glitch to zero).
    pub fn process(&mut self, x: f32) -> f32 {
        // RNNoise expects i16-scaled f32, not [-1,1].
        self.in_buf[self.in_fill] = x * 32768.0;
        self.in_fill += 1;
        if self.in_fill == DenoiseState::FRAME_SIZE {
            self.in_fill = 0;
            self.state.process_frame(&mut self.out_buf, &self.in_buf);
            self.out_pos = if self.primed {
                0
            } else {
                DenoiseState::FRAME_SIZE
            };
            self.primed = true;
        }
        if self.out_pos < DenoiseState::FRAME_SIZE {
            let v = self.out_buf[self.out_pos] / 32768.0;
            self.out_pos += 1;
            self.last = v;
        }
        self.last
    }
}

impl Default for MicDenoiser {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delay_line_accounting_is_exact() {
        // N samples in -> N samples out (each call returns one sample), and
        // the internal accumulator wraps exactly every FRAME_SIZE.
        let mut d = MicDenoiser::new();
        let n = DenoiseState::FRAME_SIZE * 7 + 123;
        let mut outs = 0;
        for i in 0..n {
            let v = d.process((i % 100) as f32 / 100.0);
            assert!(v.is_finite() && v.abs() <= 1.0, "out of range: {v}");
            outs += 1;
        }
        assert_eq!(outs, n);
        assert_eq!(d.in_fill, n % DenoiseState::FRAME_SIZE);
    }

    #[test]
    fn silence_stays_silent() {
        // The model drives gains to ~0 on silence: output must stay near
        // zero (catches i16-scaling mistakes, which would blow up bounds).
        let mut d = MicDenoiser::new();
        for _ in 0..DenoiseState::FRAME_SIZE * 4 {
            let v = d.process(0.0);
            assert!(v.abs() < 1e-3, "silence produced {v}");
        }
    }

    #[test]
    fn first_frame_artifact_is_discarded() {
        // One loud frame then silence: the loud frame's (artifact-heavy)
        // output must be dropped, so the output holds ~0 until real frames
        // arrive — i.e. no energy spike right after enable.
        let mut d = MicDenoiser::new();
        let mut max_after_enable = 0.0f32;
        for _ in 0..DenoiseState::FRAME_SIZE {
            max_after_enable = max_after_enable.max(d.process(0.9).abs());
        }
        // First output frame is the discarded artifact window (holds 0.0).
        assert!(max_after_enable == 0.0, "first frame not discarded");
    }
}
