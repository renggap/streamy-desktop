use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{CodecParameters, DecoderOptions};
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use std::fs::File;

/// Decode any symphonia-supported file into interleaved stereo-capable f32 samples.
/// Returns `(samples, sample_rate, channels)`.
pub fn decode_file(path: &str) -> Result<(Vec<f32>, u32, usize), String> {
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
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
        .map_err(|e| e.to_string())?;
    let mut format = probed.format;
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != symphonia::core::codecs::CODEC_TYPE_NULL)
        .ok_or("no playable track")?;
    let params: CodecParameters = track.codec_params.clone();
    let mut decoder = symphonia::default::get_codecs()
        .make(&params, &DecoderOptions::default())
        .map_err(|e| e.to_string())?;
    let sr = params.sample_rate.ok_or("no sample rate")?;
    let channels = params.channels.map(|c| c.count()).unwrap_or(2);

    let mut samples: Vec<f32> = Vec::new();
    let mut buf: Option<SampleBuffer<f32>> = None;
    while let Ok(packet) = format.next_packet() {
        let decoded = decoder.decode(&packet).map_err(|e| e.to_string())?;
        let spec = *decoded.spec();
        let nframes = decoded.capacity();
        if buf.is_none() {
            buf = Some(SampleBuffer::<f32>::new(nframes as u64, spec));
        }
        let sbuf = buf.as_mut().unwrap();
        sbuf.copy_interleaved_ref(decoded);
        samples.extend_from_slice(sbuf.samples());
    }
    Ok((samples, sr, channels))
}
