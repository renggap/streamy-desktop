use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::Decoder;
use symphonia::core::formats::{FormatReader, SeekMode, SeekTo};
use symphonia::core::units::Time;

use super::decoder;

/// Streaming playback voice. Holds an open symphonia decoder and decodes ahead
/// on demand. `next_frame` resamples to the output rate by reading at a
/// fractional position (linear interpolation).
///
/// Memory is bounded by a ~2 s sliding window: `trim()` drops fully-consumed
/// frames once the read head advances past the threshold, adjusting
/// `seek_offset_secs` so `position()` stays monotonic. Linear interp is fine
/// for the monitor; P2's broadcast path swaps in rubato.
pub struct Deck {
    format: Option<Box<dyn FormatReader>>,
    decoder: Option<Box<dyn Decoder>>,
    channels: usize,
    file_rate: u32,
    pcm: Vec<f32>,
    read_pos: f64,
    seek_offset_secs: f64,
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
            seek_offset_secs: 0.0,
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
        let elapsed = self.seek_offset_secs + self.read_pos / self.file_rate as f64;
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
        self.seek_offset_secs = 0.0;
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
        let target = self.total_secs * pct.clamp(0.0, 1.0);
        if let Some(format) = self.format.as_mut() {
            let to = SeekTo::Time {
                time: Time::new(target.floor() as u64, target.fract()),
                track_id: None,
            };
            // Accurate (index) seek first; fall back to coarse for index-less
            // containers (e.g. some MP3s), which seek by byte estimate.
            if let Err(e) = format.seek(SeekMode::Accurate, to) {
                let to2 = SeekTo::Time {
                    time: Time::new(target.floor() as u64, target.fract()),
                    track_id: None,
                };
                if let Err(e2) = format.seek(SeekMode::Coarse, to2) {
                    crate::diag::log(format!("seek to {target:.1}s failed: {e} / coarse: {e2}"));
                }
            }
        }
        self.pcm.clear();
        self.read_pos = 0.0;
        // Position display reflects the seek target (otherwise the bar snaps
        // back to 0 until the deck decodes past it).
        self.seek_offset_secs = target;
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
            Err(e) => {
                // Natural end of stream is an IoError(UnexpectedEof); anything else is a
                // decode problem worth logging (corrupt file, unsupported packet).
                let eof = matches!(
                    &e,
                    symphonia::core::errors::Error::IoError(io_err)
                        if io_err.kind() == std::io::ErrorKind::UnexpectedEof
                );
                if !eof {
                    crate::diag::log(format!("deck decode error (track may skip): {e:?}"));
                }
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
        self.trim();
        frame
    }

    /// Drop fully-consumed frames so memory stays O(lookahead), not O(file).
    fn trim(&mut self) {
        let threshold = self.file_rate as f64 * 2.0;
        if self.read_pos < threshold || self.channels == 0 {
            return;
        }
        let drop = self.read_pos.floor() as usize; // frames behind the read head
        let keep = self.pcm.len() / self.channels - drop;
        if keep <= 2 {
            return; // never trim below the interpolation window
        }
        self.pcm.drain(..drop * self.channels);
        self.read_pos -= drop as f64;
        self.seek_offset_secs += drop as f64 / self.file_rate as f64;
    }
}

impl Default for Deck {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Generate a mono 16-bit PCM WAV file on disk, return its path.
    /// `dur_frames` determines length; `freq` is the sine frequency in Hz.
    fn synthesize_mono_wav(dur_frames: usize, sample_rate: u32, freq: f32) -> std::path::PathBuf {
        use std::io::Write;
        let data_len = (dur_frames * 2) as u32; // 16-bit mono
        let riff_len = 36 + data_len;
        let path = std::env::temp_dir().join(format!("streamy-mono-test-{}.wav", std::process::id()));
        let mut f = std::fs::File::create(&path).unwrap();
        // RIFF header
        f.write_all(b"RIFF").unwrap();
        f.write_all(&riff_len.to_le_bytes()).unwrap();
        f.write_all(b"WAVE").unwrap();
        // fmt chunk
        f.write_all(b"fmt ").unwrap();
        f.write_all(&16u32.to_le_bytes()).unwrap(); // chunk size
        f.write_all(&1u16.to_le_bytes()).unwrap(); // PCM
        f.write_all(&1u16.to_le_bytes()).unwrap(); // mono
        f.write_all(&sample_rate.to_le_bytes()).unwrap();
        f.write_all(&(sample_rate * 2).to_le_bytes()).unwrap(); // byte rate
        f.write_all(&2u16.to_le_bytes()).unwrap(); // block align
        f.write_all(&16u16.to_le_bytes()).unwrap(); // bits per sample
        // data chunk
        f.write_all(b"data").unwrap();
        f.write_all(&data_len.to_le_bytes()).unwrap();
        for i in 0..dur_frames {
            let t = i as f32 / sample_rate as f32;
            let s = (t * freq * 2.0 * std::f32::consts::PI).sin();
            let sample = (s * 32767.0).clamp(-32768.0, 32767.0) as i16;
            f.write_all(&sample.to_le_bytes()).unwrap();
        }
        f.flush().unwrap();
        path
    }

    #[test]
    fn mono_channel_trim_and_signal_bounded() {
        // 20 s of mono 440 Hz tone at 44100 Hz.
        let path = synthesize_mono_wav(20 * 44100, 44100, 440.0);
        let mut d = Deck::new();
        d.open(path.to_str().unwrap(), 44100).unwrap();
        d.playing = true;
        assert_eq!(d.channels, 1);

        let max_allowed_frames = (d.file_rate as f64 * 3.0) as usize;
        let mut trim_count = 0u32;
        let mut prev_len = d.pcm.len();

        // Play ~17 s (at device rate = file rate, step = 1.0 → 17 × 44100 frames).
        let play_frames = 17 * 44100;
        for _ in 0..play_frames {
            let frame = d.next_frame();
            // Mono deck produces stereo output (L==R).
            assert!((frame[0] - frame[1]).abs() < 0.001, "mono deck must output L==R");
            let cur = d.pcm.len();
            if cur < prev_len {
                trim_count += 1;
            }
            prev_len = cur;
            let frames = cur / d.channels;
            assert!(
                frames <= max_allowed_frames,
                "mono pcm grew to {frames} frames"
            );
        }

        // Multiple trims must have fired (~17 s at 2 s threshold).
        assert!(trim_count > 5, "only {trim_count} trims in mono playback");
        // Signal still present after all trims.
        let mut max = 0.0f32;
        for _ in 0..2000 {
            let [l, _] = d.next_frame();
            max = max.max(l.abs());
        }
        assert!(max > 0.1, "mono signal lost after {} trims, max={max}", trim_count);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn pcm_buffer_stays_bounded_while_playing_long_track() {
        let fixture = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/tone-60s.mp3"
        );
        let mut d = Deck::new();
        d.open(fixture, 48000).unwrap();
        d.playing = true;

        let max_allowed_frames = (d.file_rate as f64 * 3.0) as usize;

        // Track monotonic position across trims.
        let mut last_elapsed = 0.0f64;
        let mut trim_count = 0u32;
        let mut prev_pcm_len = d.pcm.len();

        // Play ~30 s of output: pulls far past the trim threshold.
        for _ in 0..(30 * 48000) {
            let _ = d.next_frame();
            // Count trims by detecting pcm shrink.
            let cur = d.pcm.len();
            if cur < prev_pcm_len {
                trim_count += 1;
            }
            prev_pcm_len = cur;
            // Buffer must never exceed the bounded window (3× file rate).
            let frames = cur / d.channels;
            assert!(
                frames <= max_allowed_frames,
                "pcm grew to {frames} frames (max allowed {max_allowed_frames})"
            );
            // Position must be monotonic (no jump-back from trim).
            if let Some((_, elapsed, _)) = d.position() {
                assert!(
                    elapsed >= last_elapsed - 0.001,
                    "position regressed: {elapsed} < {last_elapsed}"
                );
                last_elapsed = elapsed;
            }
        }

        // Multiple trims must have fired (30 s at 2 s threshold → ~15 trims).
        assert!(
            trim_count > 5,
            "only {trim_count} trims — multi-trim path not exercised"
        );

        // After playing, buffered frames must stay bounded.
        let frames = d.pcm.len() / d.channels;
        assert!(
            frames <= max_allowed_frames,
            "final pcm grew to {frames} frames"
        );

        // Playback still produces signal after many trims (no window bug).
        let mut max = 0.0f32;
        for _ in 0..2000 {
            let [l, r] = d.next_frame();
            max = max.max(l.abs()).max(r.abs());
        }
        assert!(max > 0.1, "signal lost after {trim_count} trims, max={max}");
    }
}
