use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{BufferSize, SampleRate, StreamConfig};

use super::deck::Deck;

const OUT_SR: u32 = 44100;

/// Commands sent from the UI thread to the dedicated audio thread.
enum AudioCmd {
    /// Build + play the output stream (idempotent).
    Start,
    /// Drop the stream and exit the audio thread.
    Shutdown,
}

/// Send + `Sync` handle to the audio backend.
///
/// The cpal `Stream` is `!Send` on ALSA/WASAPI, so it lives entirely on a
/// dedicated audio thread; the UI thread only holds this handle and the shared
/// Deck. The plan's P0 ponytail (a `Mutex<Deck>` in Tauri state) is `!Send` and
/// cannot be `tauri::State` — so the realtime-correct ownership model the plan
/// defers to P1.2 is pulled forward here. P1.2 then extends this control channel
/// with gain/seek/source-mode actions.
pub struct AudioEngine {
    deck: Arc<Mutex<Deck>>,
    tx: mpsc::Sender<AudioCmd>,
}

impl AudioEngine {
    pub fn new() -> Self {
        let deck = Arc::new(Mutex::new(Deck::new()));
        let (tx, rx) = mpsc::channel::<AudioCmd>();
        let deck_for_thread = deck.clone();
        thread::spawn(move || audio_loop(deck_for_thread, rx));
        Self { deck, tx }
    }

    pub fn deck(&self) -> Arc<Mutex<Deck>> {
        self.deck.clone()
    }

    /// Start the output stream (builds + plays on the audio thread).
    pub fn start(&self) -> Result<(), String> {
        self.tx.send(AudioCmd::Start).map_err(|e| e.to_string())
    }
}

impl Default for AudioEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for AudioEngine {
    fn drop(&mut self) {
        let _ = self.tx.send(AudioCmd::Shutdown);
    }
}

fn audio_loop(deck: Arc<Mutex<Deck>>, rx: mpsc::Receiver<AudioCmd>) {
    let mut stream: Option<cpal::Stream> = None;
    while let Ok(cmd) = rx.recv() {
        match cmd {
            AudioCmd::Start if stream.is_none() => {
                stream = build_output_stream(deck.clone());
            }
            AudioCmd::Start => {
                if let Some(s) = stream.as_ref() {
                    let _ = s.play();
                }
            }
            AudioCmd::Shutdown => break,
        }
    }
}

fn build_output_stream(deck: Arc<Mutex<Deck>>) -> Option<cpal::Stream> {
    let host = cpal::default_host();
    let device = host.default_output_device()?;
    let config = StreamConfig {
        channels: 2,
        sample_rate: SampleRate(OUT_SR),
        buffer_size: BufferSize::Default,
    };
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
            |err: cpal::StreamError| eprintln!("cpal stream error: {err}"),
            None,
        )
        .ok()?;
    stream.play().ok()?;
    Some(stream)
}
