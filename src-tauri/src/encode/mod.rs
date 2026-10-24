#[cfg(feature = "aac")]
pub mod aac;
pub mod mp3;

pub use mp3::{bitrate_from_kbps, encode, EncoderConfig, Mp3Encoder};

use serde::{Deserialize, Serialize};

/// Output codec for the broadcast path. MP3 always builds; AAC requires the
/// `aac` cargo feature (fdk-aac).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Codec {
    Mp3,
    Aac,
}

/// A stateful encoder: one handle across `encode()` calls, flushed once at end.
/// Used by both the broadcast streamer and the (MP3) recorder.
pub trait Encoder: Send {
    fn encode(&mut self, samples: &[f32]) -> Result<Vec<u8>, String>;
    fn flush(&mut self) -> Result<Vec<u8>, String>;
    /// MIME sent to the server for this codec.
    fn content_type(&self) -> &'static str;
}

impl Encoder for Mp3Encoder {
    fn encode(&mut self, samples: &[f32]) -> Result<Vec<u8>, String> {
        Mp3Encoder::encode(self, samples)
    }
    fn flush(&mut self) -> Result<Vec<u8>, String> {
        Mp3Encoder::flush(self)
    }
    fn content_type(&self) -> &'static str {
        "audio/mpeg"
    }
}

/// Rebase an encoder onto a new device sample rate (output-device hot-swap):
/// rebuilds the handle so the encoded rate matches the PCM actually arriving.
/// No-op when the rate is unchanged (or not yet published, 0). On success the
/// caller keeps encoding; on error it keeps the old encoder and should log —
/// stale-rate audio continues rather than the stream dying.
pub fn follow_rate(
    enc: &mut Box<dyn Encoder>,
    cfg: &mut EncoderConfig,
    new_rate: u32,
) -> Result<bool, String> {
    if new_rate == 0 || new_rate == cfg.sample_rate {
        return Ok(false);
    }
    let mut next = *cfg;
    next.sample_rate = new_rate;
    let e = build(&next)?;
    *enc = e;
    *cfg = next;
    Ok(true)
}

/// Build an encoder for `cfg.codec`. AAC errors clearly when the feature is off.
pub fn build(cfg: &EncoderConfig) -> Result<Box<dyn Encoder>, String> {
    match cfg.codec {
        Codec::Mp3 => Ok(Box::new(Mp3Encoder::new(cfg)?)),
        #[cfg(feature = "aac")]
        Codec::Aac => Ok(Box::new(aac::AacEncoder::new(cfg)?)),
        #[cfg(not(feature = "aac"))]
        Codec::Aac => {
            Err("AAC encoder not built (enable the 'aac' cargo feature + fdk-aac)".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sample rate declared by the first valid MP3 frame header in `buf`
    /// (LAME output is frames back-to-back; may start with an info frame).
    fn mp3_header_rate(buf: &[u8]) -> Option<u32> {
        const MPEG1: [u32; 4] = [44100, 48000, 32000, 0];
        const MPEG2: [u32; 4] = [22050, 24000, 16000, 0];
        for w in buf.windows(4) {
            if w[0] == 0xFF && w[1] & 0xE0 == 0xE0 && w[1] & 0x06 != 0 {
                let ver = (w[1] >> 3) & 0x3; // 3 = MPEG1, 2 = MPEG2/2.5
                let idx = (w[2] >> 2) & 0x3;
                let table = if ver == 3 { MPEG1 } else { MPEG2 };
                let rate = table[idx as usize];
                // Reject reserved sampling index and free/bad bitrate fields.
                if rate != 0 && (w[2] >> 4) & 0xF != 0 && (w[2] >> 4) & 0xF != 15 {
                    return Some(rate);
                }
            }
        }
        None
    }

    fn cfg(rate: u32) -> EncoderConfig {
        EncoderConfig {
            sample_rate: rate,
            channels: 2,
            bitrate_kbps: 128,
            codec: Codec::Mp3,
        }
    }

    /// 0.2 s of interleaved stereo 440 Hz at `rate`.
    fn sine(rate: u32) -> Vec<f32> {
        (0..rate / 5)
            .flat_map(|i| {
                let s = (i as f32 * 440.0 * 2.0 * std::f32::consts::PI / rate as f32).sin();
                [s * 0.5, s * 0.5]
            })
            .collect()
    }

    #[test]
    fn follow_rate_is_noop_when_unchanged() {
        let mut enc = build(&cfg(44100)).unwrap();
        let mut c = cfg(44100);
        assert!(!follow_rate(&mut enc, &mut c, 44100).unwrap());
        // Not-yet-published rate (0) is ignored, not treated as a change.
        assert!(!follow_rate(&mut enc, &mut c, 0).unwrap());
    }

    #[test]
    fn follow_rate_rebuilds_encoder_at_new_rate() {
        // Encoder frozen at the connect-time rate while the device hot-swaps
        // is the audit's HIGH #1: encoded output must follow the new rate.
        let mut enc = build(&cfg(44100)).unwrap();
        let mut c = cfg(44100);
        let first = enc.encode(&sine(44100)).unwrap();
        assert_eq!(mp3_header_rate(&first), Some(44100));

        assert!(follow_rate(&mut enc, &mut c, 48000).unwrap());
        assert_eq!(c.sample_rate, 48000);
        let second = enc.encode(&sine(48000)).unwrap();
        assert_eq!(
            mp3_header_rate(&second),
            Some(48000),
            "encoder still declares the pre-hot-swap rate"
        );
    }
}
