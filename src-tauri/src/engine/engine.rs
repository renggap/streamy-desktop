use std::sync::{Arc, Mutex};

use cpal::{BufferSize, SampleRate, Stream, StreamConfig};

use super::deck::Deck;

const OUT_SR: u32 = 44100;

/// Owns the cpal output stream and the single Deck the callback pulls from.
///
/// ponytail: P0 guards the Deck with a `Mutex` and keeps the `Stream` in the
/// engine. P1.2 replaces this with a lock-free control queue so the realtime
/// callback is allocation/lock-free, as the design requires.
pub struct AudioEngine {
    deck: Arc<Mutex<Deck>>,
    stream: Option<Stream>,
}

impl AudioEngine {
    pub fn new() -> Self {
        Self {
            deck: Arc::new(Mutex::new(Deck::new())),
            stream: None,
        }
    }

    pub fn deck(&self) -> Arc<Mutex<Deck>> {
        self.deck.clone()
    }

    pub fn is_running(&self) -> bool {
        self.stream.is_some()
    }

    /// Open the default output device at 44100 Hz f32 stereo and begin pulling
    /// frames from the Deck.
    pub fn start(&mut self) -> Result<(), String> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| "no default output device".to_string())?;
        let config = StreamConfig {
            channels: 2,
            sample_rate: SampleRate(OUT_SR),
            buffer_size: BufferSize::Default,
        };
        let deck = self.deck.clone();
        let stream = device
            .build_output_stream(
                &config,
                move |out: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    let mut d = match deck.lock() {
                        Ok(g) => g,
                        Err(_) => return,
                    };
                    for frame in out.chunks_mut(2) {
                        let [l, r] = d.next_frame();
                        frame[0] = l;
                        if frame.len() > 1 {
                            frame[1] = r;
                        }
                    }
                },
                |err: cpal::StreamError| {
                    eprintln!("cpal stream error: {err}");
                },
                None,
            )
            .map_err(|e| e.to_string())?;
        stream.play().map_err(|e| e.to_string())?;
        self.stream = Some(stream);
        Ok(())
    }
}

impl Default for AudioEngine {
    fn default() -> Self {
        Self::new()
    }
}
