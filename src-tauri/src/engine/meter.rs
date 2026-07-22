// P1.5: pure level-meter math. Wired into the audio callback in P1.2.

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
        // mono-ish frames [0.5,0.5],[0.5,0.5] -> rms 0.5 per channel
        let r = rms(&[[0.5, 0.5], [0.5, 0.5]]);
        assert!((r[0] - 0.5).abs() < 1e-4);
        assert!((r[1] - 0.5).abs() < 1e-4);
    }

    #[test]
    fn meters_empty_is_zero() {
        assert_eq!(peak(&[]), [0.0, 0.0]);
        assert_eq!(rms(&[]), [0.0, 0.0]);
    }
}
