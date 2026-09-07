use std::fs::File;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{CodecParameters, DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::formats::{FormatOptions, FormatReader};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

/// A freshly-opened symphonia decoder ready to stream.
pub struct Opened {
    pub format: Box<dyn FormatReader>,
    pub decoder: Box<dyn symphonia::core::codecs::Decoder>,
    pub sample_rate: u32,
    pub channels: usize,
    pub total_secs: f64,
}

/// Probe a file's container + codec params (shared by `open` and `decode_file`).
fn probe(path: &str) -> Result<(Box<dyn FormatReader>, CodecParameters), String> {
    let file = File::open(path).map_err(|e| e.to_string())?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = std::path::Path::new(path)
        .extension()
        .and_then(|s| s.to_str())
    {
        hint.with_extension(ext);
    }
    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|e| e.to_string())?;
    let format = probed.format;
    let params = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .map(|t| t.codec_params.clone())
        .ok_or("no playable track")?;
    Ok((format, params))
}

/// Open a file for streaming decode. Cheap: no packets decoded yet.
pub fn open(path: &str) -> Result<Opened, String> {
    let (format, params) = probe(path)?;
    let sample_rate = params.sample_rate.ok_or("no sample rate")?;
    let channels = params.channels.map(|c| c.count()).unwrap_or(2);
    let total_secs = params
        .n_frames
        .map(|n| n as f64 / sample_rate as f64)
        .unwrap_or(0.0);
    let decoder = symphonia::default::get_codecs()
        .make(&params, &DecoderOptions::default())
        .map_err(|e| e.to_string())?;
    Ok(Opened {
        format,
        decoder,
        sample_rate,
        channels,
        total_secs,
    })
}

/// Decode an entire file into interleaved f32 (used by tests + offline paths).
pub fn decode_file(path: &str) -> Result<(Vec<f32>, u32, usize), String> {
    let (mut format, params) = probe(path)?;
    let sr = params.sample_rate.ok_or("no sample rate")?;
    let channels = params.channels.map(|c| c.count()).unwrap_or(2);
    let mut decoder = symphonia::default::get_codecs()
        .make(&params, &DecoderOptions::default())
        .map_err(|e| e.to_string())?;

    let mut samples: Vec<f32> = Vec::new();
    while let Ok(packet) = format.next_packet() {
        let decoded = decoder.decode(&packet).map_err(|e| e.to_string())?;
        let spec = *decoded.spec();
        let nframes = decoded.capacity();
        // Recreate the sample buffer per packet to avoid the capacity-assert
        // panic when a later packet is larger than the first.
        let mut sbuf = SampleBuffer::<f32>::new(nframes as u64, spec);
        sbuf.copy_interleaved_ref(decoded);
        samples.extend_from_slice(sbuf.samples());
    }
    Ok((samples, sr, channels))
}
