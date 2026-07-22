pub mod mp3;
#[cfg(feature = "aac")]
pub mod aac;

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

/// Build an encoder for `cfg.codec`. AAC errors clearly when the feature is off.
pub fn build(cfg: &EncoderConfig) -> Result<Box<dyn Encoder>, String> {
    match cfg.codec {
        Codec::Mp3 => Ok(Box::new(Mp3Encoder::new(cfg)?)),
        #[cfg(feature = "aac")]
        Codec::Aac => Ok(Box::new(aac::AacEncoder::new(cfg)?)),
        #[cfg(not(feature = "aac"))]
        Codec::Aac => Err("AAC encoder not built (enable the 'aac' cargo feature + fdk-aac)".into()),
    }
}
