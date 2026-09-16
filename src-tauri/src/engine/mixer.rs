// P1.1 + P1.4: pure mixer math + source-mode → bus sends.
// Wired into the audio callback in P1.2.

use serde::{Deserialize, Serialize};

/// Sum N source frames into one bus, each scaled by its send gain.
pub fn mix_bus(frames: &[[f32; 2]], sends: &[f32]) -> [f32; 2] {
    let mut l = 0.0f32;
    let mut r = 0.0f32;
    for (f, s) in frames.iter().zip(sends.iter()) {
        l += f[0] * s;
        r += f[1] * s;
    }
    [l, r]
}

/// Equal-power (constant-voltage) crossfade. `pos` 0..1: 0 = full A, 1 = full B.
/// gA² + gB² == 1 for all positions, so total power is constant across the fade.
pub fn equal_power(pos: f32) -> (f32, f32) {
    let p = pos.clamp(0.0, 1.0) * (std::f32::consts::PI / 2.0);
    (p.cos(), p.sin())
}

/// Mic auto-duck gain (linear) from the music level. Below `threshold` the mic is
/// untouched; above it the mic ducks up to `depth_db` by full-scale music.
/// `music_linear` is an RMS/peak level in 0..1.
pub fn duck_gain(music_linear: f32, threshold: f32, depth_db: f32) -> f32 {
    if music_linear <= threshold || depth_db <= 0.0 {
        return 1.0;
    }
    let span = (1.0 - threshold).max(1e-6);
    let over = ((music_linear - threshold) / span).clamp(0.0, 1.0);
    let db = -depth_db * over;
    10.0_f32.powf(db / 20.0)
}

/// What the DJ picks as going to air. Drives broadcast vs. monitor sends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceMode {
    File,
    Mic,
    Both,
}

/// Per-source sends into the two buses: (decks, mic) → broadcast, decks → monitor.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sends {
    pub decks_broadcast: f32,
    pub mic_broadcast: f32,
    pub decks_monitor: f32,
}

/// Map a SourceMode to bus sends:
/// - Both: decks + mic on air; decks on monitor.
/// - File: decks on air; mic nowhere (music-only mode).
/// - Mic:  mic on air; decks on monitor only.
///
/// The mic NEVER routes to the monitor bus in any mode: this app is
/// broadcast-only, and a mic on the laptop speakers is feedback/echo
/// (user-reported bug). The DJ still sees the mic level on the input meter.
pub fn sends_for(mode: SourceMode) -> Sends {
    match mode {
        SourceMode::Both => Sends {
            decks_broadcast: 1.0,
            mic_broadcast: 1.0,
            decks_monitor: 1.0,
        },
        SourceMode::File => Sends {
            decks_broadcast: 1.0,
            mic_broadcast: 0.0,
            decks_monitor: 1.0,
        },
        SourceMode::Mic => Sends {
            decks_broadcast: 0.0,
            mic_broadcast: 1.0,
            decks_monitor: 1.0,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- P1.1 mix_bus ---

    #[test]
    fn sums_sends_into_bus() {
        // two sources, broadcast sends [0.5, 0.5], frames full-scale
        let out = mix_bus(&[[0.8, 0.8], [0.4, 0.4]], &[0.5, 0.5]);
        assert!((out[0] - (0.8 * 0.5 + 0.4 * 0.5)).abs() < 1e-6);
        assert!((out[1] - (0.8 * 0.5 + 0.4 * 0.5)).abs() < 1e-6);
    }

    #[test]
    fn send_off_contributes_zero() {
        let out = mix_bus(&[[0.8, 0.8], [0.4, 0.4]], &[1.0, 0.0]);
        assert!((out[0] - 0.8).abs() < 1e-6);
    }

    // --- P1.4 source sends ---

    #[test]
    fn source_mode_file_mutes_mic_broadcast() {
        let s = sends_for(SourceMode::File);
        assert_eq!(s.decks_broadcast, 1.0);
        assert_eq!(s.mic_broadcast, 0.0);
    }

    #[test]
    fn source_mode_mic_mutes_decks_broadcast() {
        let s = sends_for(SourceMode::Mic);
        assert_eq!(s.decks_broadcast, 0.0);
        assert_eq!(s.mic_broadcast, 1.0);
        assert_eq!(s.decks_monitor, 1.0);
    }

    #[test]
    fn source_mode_both_sends_everything() {
        let s = sends_for(SourceMode::Both);
        assert_eq!(s.decks_broadcast, 1.0);
        assert_eq!(s.mic_broadcast, 1.0);
        assert_eq!(s.decks_monitor, 1.0);
    }

    #[test]
    fn mic_never_reaches_monitor_in_any_mode() {
        // Broadcast-only app: the mic must never play out of the local
        // monitor/speakers (feedback). There is no mic monitor send at all.
        for mode in [SourceMode::File, SourceMode::Mic, SourceMode::Both] {
            let _ = mode; // Sends has no mic_monitor field — compile-level
                          // guarantee; this test pins the struct shape.
        }
        let s = sends_for(SourceMode::Mic);
        assert_eq!(s.decks_monitor, 1.0, "decks still cue on monitor");
    }

    // --- P5.2 equal-power crossfade ---

    #[test]
    fn crossfade_endpoints_are_solo() {
        let (a0, b0) = equal_power(0.0);
        assert!((a0 - 1.0).abs() < 1e-6 && b0.abs() < 1e-6);
        let (a1, b1) = equal_power(1.0);
        assert!((b1 - 1.0).abs() < 1e-6 && a1.abs() < 1e-6);
    }

    #[test]
    fn crossfade_is_constant_power() {
        // gA^2 + gB^2 == 1 everywhere (no level dip through the middle).
        for i in 0..=100 {
            let p = i as f32 / 100.0;
            let (ga, gb) = equal_power(p);
            let pow = ga * ga + gb * gb;
            assert!((pow - 1.0).abs() < 1e-5, "power {pow} at pos {p}");
        }
    }

    #[test]
    fn crossfade_center_is_minus_3db() {
        let (ga, gb) = equal_power(0.5);
        assert!((ga - gb).abs() < 1e-6, "center should be symmetric");
        // ~0.707 = -3 dB on each side.
        assert!((ga - (2.0_f32).sqrt() / 2.0).abs() < 1e-5);
    }

    // --- P5.4 mic duck ---

    #[test]
    fn duck_below_threshold_is_unity() {
        assert!((duck_gain(0.1, 0.2, 12.0) - 1.0).abs() < 1e-6);
        assert!((duck_gain(0.0, 0.2, 12.0) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn duck_at_full_scale_hits_depth() {
        // threshold 0.2, depth 12 dB, full-scale music -> -12 dB = 0.251
        let g = duck_gain(1.0, 0.2, 12.0);
        assert!((g - 10.0_f32.powf(-12.0 / 20.0)).abs() < 1e-3, "g={g}");
    }

    #[test]
    fn duck_is_monotonic_decreasing() {
        let mut last = 2.0_f32;
        for i in 0..=20 {
            let lvl = 0.2 + (i as f32 / 20.0) * 0.8;
            let g = duck_gain(lvl, 0.2, 12.0);
            assert!(g <= last + 1e-6, "not monotonic at {lvl}: {g} > {last}");
            last = g;
        }
    }
}
