use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{BufferSize, SampleRate, Stream, StreamConfig};
use tauri::AppHandle;

use super::deck::Deck;
use crate::events;

const FALLBACK_RATE: u32 = 44100;

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
/// Deck. The stream is built at the **device's default config** (not a hardcoded
/// rate) so it isn't rejected by the device's shared-mode mix format.
pub struct AudioEngine {
    deck: Arc<Mutex<Deck>>,
    tx: mpsc::Sender<AudioCmd>,
    out_rate: u32,
    #[allow(dead_code)]
    out_channels: u16,
    app: Arc<Mutex<Option<AppHandle>>>,
}

impl AudioEngine {
    pub fn new() -> Self {
        let deck = Arc::new(Mutex::new(Deck::new()));
        let (tx, rx) = mpsc::channel::<AudioCmd>();
        let app: Arc<Mutex<Option<AppHandle>>> = Arc::new(Mutex::new(None));

        // Query the device's preferred output config up front so we (a) build a
        // supported stream and (b) know the rate to resample decoded PCM to.
        let host = cpal::default_host();
        let (out_rate, out_channels, stream_cfg) =
            match host.default_output_device().and_then(|d| d.default_output_config().ok()) {
                Some(c) => {
                    let rate = c.sample_rate().0;
                    let ch = c.channels();
                    let cfg: StreamConfig = c.into();
                    (rate, ch, cfg)
                }
                None => (
                    FALLBACK_RATE,
                    2,
                    StreamConfig {
                        channels: 2,
                        sample_rate: SampleRate(FALLBACK_RATE),
                        buffer_size: BufferSize::Default,
                    },
                ),
            };

        let deck_for_thread = deck.clone();
        let app_for_thread = app.clone();
        thread::spawn(move || audio_loop(deck_for_thread, rx, app_for_thread, stream_cfg));

        Self {
            deck,
            tx,
            out_rate,
            out_channels,
            app,
        }
    }

    pub fn deck(&self) -> Arc<Mutex<Deck>> {
        self.deck.clone()
    }

    /// Output sample rate decoded PCM should be resampled to before loading.
    pub fn out_rate(&self) -> u32 {
        self.out_rate
    }

    /// Hand the engine an AppHandle so its thread can emit `error` events.
    pub fn set_app(&self, handle: AppHandle) {
        if let Ok(mut slot) = self.app.lock() {
            *slot = Some(handle);
        }
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

fn audio_loop(
    deck: Arc<Mutex<Deck>>,
    rx: mpsc::Receiver<AudioCmd>,
    app: Arc<Mutex<Option<AppHandle>>>,
    cfg: StreamConfig,
) {
    let mut stream: Option<Stream> = None;
    while let Ok(cmd) = rx.recv() {
        match cmd {
            AudioCmd::Start if stream.is_none() => match build_output_stream(deck.clone(), &cfg, app.clone()) {
                Ok(s) => stream = Some(s),
                Err(e) => emit_err(&app, format!("audio start failed: {e}")),
            },
            AudioCmd::Start => {
                if let Some(s) = stream.as_ref() {
                    let _ = s.play();
                }
            }
            AudioCmd::Shutdown => break,
        }
    }
}

fn emit_err(app: &Arc<Mutex<Option<AppHandle>>>, msg: String) {
    let handle = app.lock().ok().and_then(|g| g.clone());
    if let Some(h) = handle {
        events::emit_error(&h, msg);
    } else {
        eprintln!("[audio] {msg}");
    }
}

/// Write a stereo frame into an N-channel output slice.
fn write_frame(buf: &mut [f32], channels: usize, l: f32, r: f32) {
    match channels {
        0 => {}
        1 => buf[0] = 0.5 * (l + r),
        n => {
            buf[0] = l;
            if n > 1 {
                buf[1] = r;
            }
            for s in buf.iter_mut().take(n).skip(2) {
                *s = 0.0;
            }
        }
    }
}

fn build_output_stream(
    deck: Arc<Mutex<Deck>>,
    cfg: &StreamConfig,
    app: Arc<Mutex<Option<AppHandle>>>,
) -> Result<Stream, String> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or_else(|| "no default output device".to_string())?;
    let channels = cfg.channels as usize;

    let stream = device
        .build_output_stream(
            cfg,
            move |out: &mut [f32], _: &cpal::OutputCallbackInfo| {
                let mut d = match deck.lock() {
                    Ok(g) => g,
                    Err(_) => return,
                };
                for frame in out.chunks_mut(channels.max(1)) {
                    let [l, r] = d.next_frame();
                    write_frame(frame, frame.len(), l, r);
                }
            },
            move |err: cpal::StreamError| {
                emit_err(&app, format!("audio stream error: {err}"));
            },
            None,
        )
        .map_err(|e| e.to_string())?;
    stream.play().map_err(|e| e.to_string())?;
    Ok(stream)
}
