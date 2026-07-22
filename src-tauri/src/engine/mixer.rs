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

/// What the DJ picks as going to air. Drives broadcast vs. monitor sends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceMode {
    File,
    Mic,
    Both,
}

/// Per-source sends into the two buses. Each of (decks, mic) × (broadcast, monitor).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sends {
    pub decks_broadcast: f32,
    pub mic_broadcast: f32,
    pub decks_monitor: f32,
    pub mic_monitor: f32,
}

/// Map a SourceMode to bus sends:
/// - Both: everything wide open.
/// - File: decks to both buses; mic to monitor only (cue, not on air).
/// - Mic:  mic to both buses; decks to monitor only.
pub fn sends_for(mode: SourceMode) -> Sends {
    match mode {
        SourceMode::Both => Sends {
            decks_broadcast: 1.0,
            mic_broadcast: 1.0,
            decks_monitor: 1.0,
            mic_monitor: 1.0,
        },
        SourceMode::File => Sends {
            decks_broadcast: 1.0,
            mic_broadcast: 0.0,
            decks_monitor: 1.0,
            mic_monitor: 1.0,
        },
        SourceMode::Mic => Sends {
            decks_broadcast: 0.0,
            mic_broadcast: 1.0,
            decks_monitor: 1.0,
            mic_monitor: 1.0,
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
        // mic still on monitor in File mode:
        assert_eq!(s.mic_monitor, 1.0);
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
        assert_eq!(s.mic_monitor, 1.0);
    }
}
