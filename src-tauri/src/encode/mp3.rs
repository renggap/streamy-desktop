// P2.1: MP3 encoding via the vendored libmp3lame, compiled by build.rs with the
// cc crate. Direct minimal FFI (no extra Rust crate), normalized to stereo i16
// — lame's native path.

use std::os::raw::c_int;

#[repr(C)]
struct LameGlobalFlags {
    _opaque: [u8; 0],
}

extern "C" {
    fn lame_init() -> *mut LameGlobalFlags;
    fn lame_set_num_channels(gfp: *mut LameGlobalFlags, n: c_int) -> c_int;
    fn lame_set_in_samplerate(gfp: *mut LameGlobalFlags, rate: c_int) -> c_int;
    fn lame_set_brate(gfp: *mut LameGlobalFlags, brate: c_int) -> c_int;
    fn lame_init_params(gfp: *mut LameGlobalFlags) -> c_int;
    fn lame_encode_buffer_interleaved(
        gfp: *mut LameGlobalFlags,
        pcm: *const i16,
        num_samples: c_int,
        mp3buf: *mut u8,
        mp3buf_size: c_int,
    ) -> c_int;
    fn lame_encode_flush(gfp: *mut LameGlobalFlags, mp3buf: *mut u8, mp3buf_size: c_int) -> c_int;
    fn lame_close(gfp: *mut LameGlobalFlags) -> c_int;
}

#[derive(Debug, Clone)]
pub struct EncoderConfig {
    pub sample_rate: u32,
    pub channels: u8,
    pub bitrate_kbps: u32,
}

const SUPPORTED_BITRATES: [u32; 7] = [64, 96, 128, 160, 192, 256, 320];

/// Validate / return a supported kbps preset.
pub fn bitrate_from_kbps(kbps: u32) -> Result<u32, String> {
    if SUPPORTED_BITRATES.contains(&kbps) {
        Ok(kbps)
    } else {
        Err(format!("unsupported bitrate: {kbps} kbps"))
    }
}

fn check(v: c_int) -> Result<(), String> {
    if v < 0 {
        Err(format!("lame error code {v}"))
    } else {
        Ok(())
    }
}

/// Encode interleaved f32 PCM to MP3 bytes. Mono input is up-mixed to stereo.
pub fn encode(samples: &[f32], cfg: &EncoderConfig) -> Result<Vec<u8>, String> {
    bitrate_from_kbps(cfg.bitrate_kbps)?;

    // Normalize to interleaved stereo.
    let stereo: Vec<f32> = match cfg.channels {
        1 => samples.iter().flat_map(|&s| [s, s]).collect(),
        _ => samples.to_vec(),
    };
    let pcm: Vec<i16> = stereo
        .iter()
        .map(|&s| (s.clamp(-1.0, 1.0) * 32767.0) as i16)
        .collect();
    let num_samples = (pcm.len() / 2) as c_int; // frames

    unsafe {
        let gfp = lame_init();
        if gfp.is_null() {
            return Err("lame_init failed".to_string());
        }

        let result = (|| -> Result<Vec<u8>, String> {
            check(lame_set_num_channels(gfp, 2))?;
            check(lame_set_in_samplerate(gfp, cfg.sample_rate as c_int))?;
            check(lame_set_brate(gfp, cfg.bitrate_kbps as c_int))?;
            check(lame_init_params(gfp))?;

            let bufsize = ((num_samples as f64) * 1.25 + 7200.0) as usize + 16;
            let mut buf = vec![0u8; bufsize];
            let mut out = Vec::new();

            if num_samples > 0 {
                let n = lame_encode_buffer_interleaved(
                    gfp,
                    pcm.as_ptr(),
                    num_samples,
                    buf.as_mut_ptr(),
                    bufsize as c_int,
                );
                if n < 0 {
                    return Err(format!("lame_encode error {n}"));
                }
                out.extend_from_slice(&buf[..n as usize]);
            }

            let n = lame_encode_flush(gfp, buf.as_mut_ptr(), bufsize as c_int);
            if n < 0 {
                return Err(format!("lame_flush error {n}"));
            }
            out.extend_from_slice(&buf[..n as usize]);
            Ok(out)
        })();

        lame_close(gfp);
        result
    }
}
