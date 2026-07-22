// P1.5: level-meter math + realtime accumulator. Wired into the audio callback.

use serde::{Deserialize, Serialize};

/// One meter snapshot emitted on the `levels` event: monitor (what the DJ
/// hears) + mic (input), per channel. broadcast joins in P2.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct LevelMeter {
    pub monitor: [f32; 2],
    pub mic: [f32; 2],
}

/// Realtime meter accumulator: running per-channel peak since the last emit,
/// emitted ~30 fps. No allocation in the audio callback.
pub struct Meter {
    sample_rate: u32,
    acc_mon: [f32; 2],
    acc_mic: [f32; 2],
    frames: usize,
    emit_every: usize,
}

impl Meter {
    pub fn new(sample_rate: u32) -> Self {
        Self {
            sample_rate,
            acc_mon: [0.0, 0.0],
            acc_mic: [0.0, 0.0],
            frames: 0,
            emit_every: emit_window(sample_rate),
        }
    }

    pub fn set_sample_rate(&mut self, sample_rate: u32) {
        self.sample_rate = sample_rate;
        self.emit_every = emit_window(sample_rate);
    }

    pub fn rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn push(&mut self, mon: [f32; 2], mic: [f32; 2]) {
        self.acc_mon[0] = self.acc_mon[0].max(mon[0].abs());
        self.acc_mon[1] = self.acc_mon[1].max(mon[1].abs());
        self.acc_mic[0] = self.acc_mic[0].max(mic[0].abs());
        self.acc_mic[1] = self.acc_mic[1].max(mic[1].abs());
        self.frames += 1;
    }

    pub fn take(&mut self) -> Option<LevelMeter> {
        if self.frames >= self.emit_every {
            let m = LevelMeter {
                monitor: self.acc_mon,
                mic: self.acc_mic,
            };
            self.acc_mon = [0.0, 0.0];
            self.acc_mic = [0.0, 0.0];
            self.frames = 0;
            Some(m)
        } else {
            None
        }
    }
}

fn emit_window(sample_rate: u32) -> usize {
    ((sample_rate as f64 / 30.0).round() as usize).max(1)
}

/// Per-channel peak (max |sample|) over a window of stereo frames.
pub fn peak(frames: &[[f32; 2]]) -> [f32; 2] {
    let mut mx = [0.0f32; 2];
    for f in frames {
        for c in 0..2 {
            let v = f[c].abs();
            if v > mx[c] {
                mx[c] = v;
            }
        }
    }
    mx
}

/// Per-channel RMS over a window of stereo frames.
pub fn rms(frames: &[[f32; 2]]) -> [f32; 2] {
    let n = frames.len();
    if n == 0 {
        return [0.0, 0.0];
    }
    let mut s = [0.0f64; 2];
    for f in frames {
        for c in 0..2 {
            s[c] += (f[c] as f64) * (f[c] as f64);
        }
    }
    [
        (s[0] / n as f64).sqrt() as f32,
        (s[1] / n as f64).sqrt() as f32,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peak_picks_max_abs_per_channel() {
        let p = peak(&[[0.5, 0.5], [0.9, 0.1]]);
        assert!((p[0] - 0.9).abs() < 1e-6);
        assert!((p[1] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn peak_handles_negative() {
        let p = peak(&[[-0.8, 0.2], [0.3, -0.6]]);
        assert!((p[0] - 0.8).abs() < 1e-6);
        assert!((p[1] - 0.6).abs() < 1e-6);
    }

    #[test]
    fn rms_matches_definition() {
        let r = rms(&[[0.5, 0.5], [0.5, 0.5]]);
        assert!((r[0] - 0.5).abs() < 1e-4);
        assert!((r[1] - 0.5).abs() < 1e-4);
    }

    #[test]
    fn meters_empty_is_zero() {
        assert_eq!(peak(&[]), [0.0, 0.0]);
        assert_eq!(rms(&[]), [0.0, 0.0]);
    }

    #[test]
    fn meter_accumulates_and_emits() {
        let mut m = Meter::new(30); // emit_every = 1 frame
        m.push([0.4, 0.6], [0.2, 0.8]);
        let l = m.take().unwrap();
        assert!((l.monitor[0] - 0.4).abs() < 1e-6);
        assert!((l.monitor[1] - 0.6).abs() < 1e-6);
        assert!((l.mic[1] - 0.8).abs() < 1e-6);
        // resets after take
        assert!(m.take().is_none());
    }
}
