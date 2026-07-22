// P4.1: AAC encoder via fdk-aac (feature `aac`). Outputs ADTS-framed AAC-LC.
//
// fdk-aac consumes interleaved i16 samples in fixed 1024-frame blocks; it buffers
// a lookahead internally, so early encode() calls may emit nothing and a final
// flush() drains the tail. Our Encoder trait wraps that behind the same
// encode/flush shape as MP3.

use fdk_aac::enc::{AudioObjectType, BitRate, ChannelMode, Encoder as FdkEncoder, EncoderParams, Transport};

use super::Encoder;
use crate::encode::EncoderConfig;

pub struct AacEncoder {
    enc: FdkEncoder,
    out: Vec<u8>,
}

impl AacEncoder {
    pub fn new(cfg: &EncoderConfig) -> Result<Self, String> {
        let params = EncoderParams {
            bit_rate: BitRate::Cbr(cfg.bitrate_kbps * 1000),
            sample_rate: cfg.sample_rate,
            transport: Transport::Adts,
            channels: ChannelMode::Stereo,
            audio_object_type: AudioObjectType::Mpeg4LowComplexity,
        };
        let enc = FdkEncoder::new(params).map_err(|e| format!("fdk-aac: {e:?}"))?;
        // 6144 bytes/channel/frame is the AAC max; stereo + ADTS header fits in 16KB.
        Ok(Self { enc, out: vec![0u8; 16384] })
    }
}

impl Encoder for AacEncoder {
    fn encode(&mut self, samples: &[f32]) -> Result<Vec<u8>, String> {
        // f32 interleaved -> i16 interleaved.
        let pcm: Vec<i16> = samples
            .iter()
            .map(|&s| (s.clamp(-1.0, 1.0) * 32767.0).round() as i16)
            .collect();
        let mut out = Vec::new();
        let mut pos = 0;
        while pos < pcm.len() {
            let info = self
                .enc
                .encode(&pcm[pos..], &mut self.out)
                .map_err(|e| format!("fdk-aac: {e:?}"))?;
            if info.output_size > 0 {
                out.extend_from_slice(&self.out[..info.output_size]);
            }
            let consumed = info.input_consumed;
            if consumed == 0 {
                break; // need more input than we have; remainder flushed at end
            }
            pos += consumed;
        }
        Ok(out)
    }

    fn flush(&mut self) -> Result<Vec<u8>, String> {
        // Drain the encoder's lookahead with empty-input calls until it stops.
        let mut out = Vec::new();
        loop {
            let info = self
                .enc
                .encode(&[], &mut self.out)
                .map_err(|e| format!("fdk-aac: {e:?}"))?;
            if info.output_size > 0 {
                out.extend_from_slice(&self.out[..info.output_size]);
            } else {
                break;
            }
        }
        Ok(out)
    }

    fn content_type(&self) -> &'static str {
        "audio/aacp"
    }
}
