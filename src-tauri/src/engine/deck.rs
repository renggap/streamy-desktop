use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::Decoder;
use symphonia::core::formats::{FormatReader, SeekMode, SeekTo};
use symphonia::core::units::Time;

use super::decoder;

/// Streaming playback voice. Holds an open symphonia decoder and decodes ahead
/// on demand. `next_frame` resamples to the output rate by reading at a
/// fractional position (linear interpolation).
///
/// ponytail: linear interp is fine for the monitor; P2's broadcast path swaps
/// in rubato. pcm grows to the whole file (same memory as P0); a sliding
/// window trim is a later optimization.
pub struct Deck {
    format: Option<Box<dyn FormatReader>>,
    decoder: Option<Box<dyn Decoder>>,
    channels: usize,
    file_rate: u32,
    pcm: Vec<f32>,
    read_pos: f64,
    step: f64,
    total_secs: f64,
    path: Option<String>,
    pub playing: bool,
    ended: bool,
    just_ended: bool,
}

impl Deck {
    pub fn new() -> Self {
        Self {
            format: None,
            decoder: None,
            channels: 2,
            file_rate: 44100,
            pcm: Vec::new(),
            read_pos: 0.0,
            step: 1.0,
            total_secs: 0.0,
            path: None,
            playing: false,
            ended: false,
            just_ended: false,
        }
    }

    /// Recompute the resample step for a new output device rate (keeps position).
    pub fn set_device_rate(&mut self, device_rate: u32) {
        self.step = if device_rate == 0 {
            1.0
        } else {
            self.file_rate as f64 / device_rate as f64
        };
    }

    /// Returns true once when the track finishes naturally (distinct from pause).
    pub fn take_ended(&mut self) -> bool {
        let e = self.just_ended;
        self.just_ended = false;
        e
    }

    /// Playback position: (pct 0..1, elapsed_secs, total_secs). None if no track.
    pub fn position(&self) -> Option<(f64, f64, f64)> {
        if self.file_rate == 0 || self.total_secs <= 0.0 {
            return None;
        }
        let elapsed = self.read_pos / self.file_rate as f64;
        Some((elapsed / self.total_secs, elapsed, self.total_secs))
    }

    /// Open a file for streaming at the output device's rate. Cheap; no decode.
    /// Does not touch `playing` — caller decides.
    pub fn open(&mut self, path: &str, device_rate: u32) -> Result<(), String> {
        let o = decoder::open(path)?;
        self.format = Some(o.format);
        self.decoder = Some(o.decoder);
        self.channels = o.channels.max(1);
        self.file_rate = o.sample_rate;
        self.step = if device_rate == 0 {
            1.0
        } else {
            o.sample_rate as f64 / device_rate as f64
        };
        self.total_secs = o.total_secs;
        self.path = Some(path.to_string());
        self.pcm.clear();
        self.read_pos = 0.0;
        self.ended = false;
        self.just_ended = false;
        Ok(())
    }

    /// Seek to `pct` of the track. Reopens the file (cheap reset) then seeks.
    pub fn seek(&mut self, pct: f64, device_rate: u32) {
        let path = match self.path.clone() {
            Some(p) => p,
            None => return,
        };
        if self.open(&path, device_rate).is_err() {
            return;
        }
        if let Some(format) = self.format.as_mut() {
            let target = self.total_secs * pct.clamp(0.0, 1.0);
            let to = SeekTo::Time {
                time: Time::new(target.floor() as u64, target.fract()),
                track_id: None,
            };
            let _ = format.seek(SeekMode::Accurate, to);
        }
        self.pcm.clear();
        self.read_pos = 0.0;
        self.ended = false;
    }

    /// Decode ahead until at least `read_pos + 2` frames are buffered.
    fn refill(&mut self) {
        let needed = (self.read_pos as usize) + 2;
        while self.pcm.len() / self.channels <= needed {
            if !self.next_packet() {
                break;
            }
        }
    }

    fn next_packet(&mut self) -> bool {
        let (format, decoder) = match (self.format.as_mut(), self.decoder.as_mut()) {
            (Some(f), Some(d)) => (f, d),
            _ => return false,
        };
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(_) => {
                self.ended = true;
                return false;
            }
        };
        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            Err(_) => return false,
        };
        let spec = *decoded.spec();
        let nframes = decoded.capacity();
        let mut sbuf = SampleBuffer::<f32>::new(nframes as u64, spec);
        sbuf.copy_interleaved_ref(decoded);
        self.pcm.extend_from_slice(sbuf.samples());
        true
    }

    /// Next stereo frame at the output rate (resampled on read).
    pub fn next_frame(&mut self) -> [f32; 2] {
        if !self.playing || self.format.is_none() || self.channels == 0 {
            return [0.0, 0.0];
        }
        self.refill();
        let c = self.channels;
        let nframes = self.pcm.len() / c;
        if nframes == 0 {
            return [0.0, 0.0];
        }
        let p = self.read_pos;
        let i0 = (p as usize).min(nframes - 1);
        let i1 = (i0 + 1).min(nframes - 1);
        let frac = (p - i0 as f64) as f32;
        let frame = match c {
            1 => {
                let s0 = self.pcm[i0];
                let s1 = self.pcm[i1];
                let m = s0 + (s1 - s0) * frac;
                [m, m]
            }
            _ => {
                let l = self.pcm[i0 * 2] + (self.pcm[i1 * 2] - self.pcm[i0 * 2]) * frac;
                let r = self.pcm[i0 * 2 + 1] + (self.pcm[i1 * 2 + 1] - self.pcm[i0 * 2 + 1]) * frac;
                [l, r]
            }
        };
        self.read_pos += self.step;
        if self.read_pos >= nframes as f64 && self.ended {
            self.playing = false;
            self.read_pos = 0.0;
            self.just_ended = true;
        }
        frame
    }
}

impl Default for Deck {
    fn default() -> Self {
        Self::new()
    }
}
