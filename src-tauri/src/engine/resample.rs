/// Linear-interpolation resampler.
///
/// ponytail: linear interp is fine for P0 monitoring; P1 swaps in rubato
/// (band-limited) per the design for broadcast quality.
pub fn resample_linear(
    input: &[f32],
    channels: usize,
    in_rate: u32,
    out_rate: u32,
) -> Vec<f32> {
    if input.is_empty() || channels == 0 {
        return Vec::new();
    }
    if in_rate == out_rate {
        return input.to_vec();
    }
    let n_in = input.len() / channels;
    if n_in == 0 {
        return Vec::new();
    }
    let n_out = (((n_in as f64) * (out_rate as f64) / (in_rate as f64)).round() as usize).max(1);
    let step = (n_in as f64) / (n_out as f64); // input frames per output frame
    let mut out = Vec::with_capacity(n_out * channels);
    for i in 0..n_out {
        let pos = i as f64 * step;
        let i0 = pos as usize;
        let i1 = (i0 + 1).min(n_in - 1);
        let frac = (pos - i0 as f64) as f32;
        for c in 0..channels {
            let s0 = input[i0 * channels + c];
            let s1 = input[i1 * channels + c];
            out.push(s0 + (s1 - s0) * frac);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_when_rates_match() {
        let input = vec![0.0f32, 0.1, 0.2, 0.3];
        let out = resample_linear(&input, 2, 44100, 44100);
        assert_eq!(out, input);
    }

    #[test]
    fn upsample_doubles_frames_and_preserves_dc() {
        // 4 stereo frames of constant 0.5 -> 8 frames at 2x rate, still 0.5
        let input: Vec<f32> = (0..4).flat_map(|_| [0.5f32, 0.5]).collect();
        let out = resample_linear(&input, 2, 44100, 88200);
        assert_eq!(out.len(), 16); // 8 frames * 2 ch
        assert!(out.iter().all(|&s| (s - 0.5).abs() < 1e-6));
    }

    #[test]
    fn linear_interp_on_ramp() {
        // mono ramp [0,1,2,3] @ 4 frames -> 8 frames @ 2x
        let input = vec![0.0f32, 1.0, 2.0, 3.0];
        let out = resample_linear(&input, 1, 44100, 88200);
        assert_eq!(out.len(), 8);
        let expected = [0.0, 0.5, 1.0, 1.5, 2.0, 2.5, 3.0, 3.0];
        for (i, (got, want)) in out.iter().zip(expected.iter()).enumerate() {
            assert!((got - want).abs() < 1e-6, "idx {i}: got {got} want {want}");
        }
    }

    #[test]
    fn downsample_halves_frames() {
        let input: Vec<f32> = (0..8).map(|i| i as f32).collect(); // 8 mono frames
        let out = resample_linear(&input, 1, 88200, 44100);
        assert_eq!(out.len(), 4);
    }

    #[test]
    fn empty_in_empty_out() {
        assert!(resample_linear(&[], 2, 44100, 48000).is_empty());
    }
}
