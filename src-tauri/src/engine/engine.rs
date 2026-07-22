use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{BufferSize, SampleRate, Stream, StreamConfig};
use tauri::{AppHandle, Emitter};

use super::deck::Deck;
use super::meter::Meter;
use super::mic::MicInput;
use super::mixer::{mix_bus, sends_for, SourceMode};
use crate::events;

const FALLBACK_RATE: u32 = 44100;

/// Control messages pushed from the UI thread and drained at the top of each
/// audio callback. Deck state lives entirely on the audio (cpal render) thread,
/// so the callback is lock-free except for this lock-free queue.
pub enum Action {
    Load(String),
    Play,
    Pause,
    Seek(f64),
    SourceMode(SourceMode),
}

/// Send + `Sync` handle to the audio backend. Owns a control-queue producer;
/// the `!Send` cpal `Stream` lives on a dedicated thread.
pub struct AudioEngine {
    control: Mutex<rtrb::Producer<Action>>,
    shutdown: mpsc::Sender<()>,
    app: Arc<Mutex<Option<AppHandle>>>,
    mic: MicInput,
}

impl AudioEngine {
    pub fn new() -> Self {
        let host = cpal::default_host();
        let cfg = match host
            .default_output_device()
            .and_then(|d| d.default_output_config().ok())
        {
            Some(c) => StreamConfig::from(c),
            None => StreamConfig {
                channels: 2,
                sample_rate: SampleRate(FALLBACK_RATE),
                buffer_size: BufferSize::Default,
            },
        };

        let (prod, cons) = rtrb::RingBuffer::<Action>::new(64);
        let (shutdown_tx, shutdown_rx) = mpsc::channel::<()>();
        let app: Arc<Mutex<Option<AppHandle>>> = Arc::new(Mutex::new(None));
        let app_for_thread = app.clone();

        // Mic ring: producer → mic input thread, consumer → output callback.
        let (mic_prod, mic_cons) = rtrb::RingBuffer::<[f32; 2]>::new(8192);
        let mic_prod = Arc::new(Mutex::new(mic_prod));
        let mic = MicInput::new(mic_prod);

        thread::spawn(move || audio_loop(cons, shutdown_rx, app_for_thread, cfg, mic_cons));

        Self {
            control: Mutex::new(prod),
            shutdown: shutdown_tx,
            app,
            mic,
        }
    }

    /// Switch the mic input device by name (re-opens on the mic thread).
    pub fn set_input_device(&self, name: String) -> Result<(), String> {
        self.mic.set_device(name)
    }

    /// Hand the engine an AppHandle so its thread can emit `error`/`levels`.
    pub fn set_app(&self, handle: AppHandle) {
        if let Ok(mut slot) = self.app.lock() {
            *slot = Some(handle);
        }
    }

    fn push(&self, a: Action) -> Result<(), String> {
        let mut p = self.control.lock().map_err(|e| e.to_string())?;
        p.push(a).map_err(|_| "audio control queue full".to_string())
    }
    pub fn load(&self, path: String) -> Result<(), String> {
        self.push(Action::Load(path))
    }
    pub fn play(&self) -> Result<(), String> {
        self.push(Action::Play)
    }
    pub fn pause(&self) -> Result<(), String> {
        self.push(Action::Pause)
    }
    pub fn seek(&self, pct: f64) -> Result<(), String> {
        self.push(Action::Seek(pct))
    }
    pub fn set_source_mode(&self, m: SourceMode) -> Result<(), String> {
        self.push(Action::SourceMode(m))
    }
}

impl Default for AudioEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for AudioEngine {
    fn drop(&mut self) {
        let _ = self.shutdown.send(());
    }
}

fn audio_loop(
    cons: rtrb::Consumer<Action>,
    shutdown: mpsc::Receiver<()>,
    app: Arc<Mutex<Option<AppHandle>>>,
    cfg: StreamConfig,
    mic_cons: rtrb::Consumer<[f32; 2]>,
) {
    let stream = match build_output_stream(cons, app.clone(), cfg, mic_cons) {
        Ok(s) => {
            let _ = s.play();
            s
        }
        Err(e) => {
            emit_err(&app, format!("audio start failed: {e}"));
            let _ = shutdown.recv();
            return;
        }
    };
    // Hold the stream alive until shutdown.
    let _ = shutdown.recv();
    drop(stream);
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
fn write_frame(buf: &mut [f32], l: f32, r: f32) {
    match buf.len() {
        0 => {}
        1 => buf[0] = 0.5 * (l + r),
        n => {
            buf[0] = l;
            buf[1] = r;
            for s in buf.iter_mut().take(n).skip(2) {
                *s = 0.0;
            }
        }
    }
}

fn build_output_stream(
    mut cons: rtrb::Consumer<Action>,
    app: Arc<Mutex<Option<AppHandle>>>,
    cfg: StreamConfig,
    mut mic_cons: rtrb::Consumer<[f32; 2]>,
) -> Result<Stream, String> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or_else(|| "no default output device".to_string())?;
    let channels = cfg.channels as usize;
    let device_rate = cfg.sample_rate.0;

    let mut deck = Deck::new();
    let mut meter = Meter::new(device_rate);
    let mut source_mode = SourceMode::File;
    let app_for_err = app.clone();

    let stream = device
        .build_output_stream(
            &cfg,
            move |out: &mut [f32], _: &cpal::OutputCallbackInfo| {
                // 1. Drain control queue (Load/Play/Pause/Seek/SourceMode).
                while let Ok(action) = cons.pop() {
                    match action {
                        Action::Load(p) => match deck.open(&p, device_rate) {
                            Ok(()) => deck.playing = true,
                            Err(e) => emit_err(&app, format!("load failed: {e}")),
                        },
                        Action::Play => deck.playing = true,
                        Action::Pause => deck.playing = false,
                        Action::Seek(pct) => deck.seek(pct, device_rate),
                        Action::SourceMode(m) => source_mode = m,
                    }
                }

                // 2. Produce monitor frames (deck + mic[P1.3]) + meter.
                let sends = sends_for(source_mode);
                for frame in out.chunks_mut(channels.max(1)) {
                    let deck_frame = deck.next_frame();
                    let mic_frame = mic_cons.pop().unwrap_or([0.0, 0.0]);
                    let mon = mix_bus(
                        &[deck_frame, mic_frame],
                        &[sends.decks_monitor, sends.mic_monitor],
                    );
                    meter.push(mon);
                    write_frame(frame, mon[0], mon[1]);
                }

                // 3. Emit levels ~30 fps.
                if let Some(levels) = meter.take() {
                    if let Some(h) = app.lock().ok().and_then(|g| g.clone()) {
                        let _ = h.emit(events::LEVELS, levels);
                    }
                }

                // 4. Signal track end so the UI can auto-advance.
                if deck.take_ended() {
                    if let Some(h) = app.lock().ok().and_then(|g| g.clone()) {
                        let _ = h.emit(events::TRACK_ENDED, ());
                    }
                }
            },
            move |err: cpal::StreamError| emit_err(&app_for_err, format!("audio stream error: {err}")),
            None,
        )
        .map_err(|e| e.to_string())?;
    Ok(stream)
}
