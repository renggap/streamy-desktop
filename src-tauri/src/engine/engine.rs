use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{BufferSize, SampleRate, Stream, StreamConfig};
use tauri::{AppHandle, Emitter};

use super::deck::Deck;
use super::meter::Meter;
use super::mic::MicInput;
use super::mixer::{mix_bus, sends_for, SourceMode};
use crate::events;

const FALLBACK_RATE: u32 = 44100;

/// Control messages pushed from the UI thread, drained at the top of each
/// audio callback.
pub enum Action {
    Load(String),
    Play,
    Pause,
    Seek(f64),
    SourceMode(SourceMode),
}

/// Signals to the audio loop: rebuild the stream (e.g. output device lost) or
/// shut down.
enum Signal {
    Rebuild,
    Shutdown,
}

/// All realtime state, shared across stream rebuilds so a device hot-swap
/// (headphone unplug) can rebuild the stream without losing playback.
struct EngineState {
    deck: Deck,
    meter: Meter,
    source_mode: SourceMode,
    cons: rtrb::Consumer<Action>,
    mic_cons: rtrb::Consumer<[f32; 2]>,
}

/// Send + `Sync` handle to the audio backend.
pub struct AudioEngine {
    control: Mutex<rtrb::Producer<Action>>,
    sig: mpsc::Sender<Signal>,
    app: Arc<Mutex<Option<AppHandle>>>,
    mic: MicInput,
}

impl AudioEngine {
    pub fn new() -> Self {
        let (prod, cons) = rtrb::RingBuffer::<Action>::new(64);
        let (mic_prod, mic_cons) = rtrb::RingBuffer::<[f32; 2]>::new(8192);
        let mic_prod = Arc::new(Mutex::new(mic_prod));
        let mic = MicInput::new(mic_prod);

        let (sig_tx, sig_rx) = mpsc::channel::<Signal>();
        let app: Arc<Mutex<Option<AppHandle>>> = Arc::new(Mutex::new(None));
        let state = Arc::new(Mutex::new(EngineState {
            deck: Deck::new(),
            meter: Meter::new(FALLBACK_RATE),
            source_mode: SourceMode::File,
            cons,
            mic_cons,
        }));

        let app_for_thread = app.clone();
        let state_for_thread = state.clone();
        let sig_for_thread = sig_tx.clone();
        thread::spawn(move || audio_loop(state_for_thread, app_for_thread, sig_for_thread, sig_rx));

        Self {
            control: Mutex::new(prod),
            sig: sig_tx,
            app,
            mic,
        }
    }

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
    pub fn set_input_device(&self, name: String) -> Result<(), String> {
        self.mic.set_device(name)
    }
}

impl Default for AudioEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for AudioEngine {
    fn drop(&mut self) {
        let _ = self.sig.send(Signal::Shutdown);
    }
}

fn audio_loop(
    state: Arc<Mutex<EngineState>>,
    app: Arc<Mutex<Option<AppHandle>>>,
    sig_tx: mpsc::Sender<Signal>,
    sig_rx: mpsc::Receiver<Signal>,
) {
    loop {
        let cfg = current_output_cfg();
        // Adapt meter window + deck resample step to this device's rate.
        if let Ok(mut s) = state.lock() {
            s.meter.set_sample_rate(cfg.sample_rate.0);
            s.deck.set_device_rate(cfg.sample_rate.0);
        }
        match build_output_stream(state.clone(), app.clone(), sig_tx.clone(), &cfg) {
            Ok(stream) => {
                let _ = stream.play();
                // Hold the stream until a rebuild (device lost) or shutdown.
                match sig_rx.recv() {
                    Ok(Signal::Rebuild) => continue, // drop stream, rebuild on new default
                    _ => break,
                }
            }
            Err(e) => {
                emit_err(&app, format!("audio start failed: {e}"));
                thread::sleep(Duration::from_secs(1)); // no device yet; retry
            }
        }
    }
}

fn current_output_cfg() -> StreamConfig {
    let host = cpal::default_host();
    match host
        .default_output_device()
        .and_then(|d| d.default_output_config().ok())
    {
        Some(c) => StreamConfig::from(c),
        None => StreamConfig {
            channels: 2,
            sample_rate: SampleRate(FALLBACK_RATE),
            buffer_size: BufferSize::Default,
        },
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
    state: Arc<Mutex<EngineState>>,
    app: Arc<Mutex<Option<AppHandle>>>,
    sig_tx: mpsc::Sender<Signal>,
    cfg: &StreamConfig,
) -> Result<Stream, String> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or_else(|| "no default output device".to_string())?;
    let channels = cfg.channels as usize;
    let device_rate = cfg.sample_rate.0;

    let stream = device
        .build_output_stream(
            cfg,
            move |out: &mut [f32], _: &cpal::OutputCallbackInfo| {
                let (levels, ended) = {
                    let mut s = match state.lock() {
                        Ok(g) => g,
                        Err(_) => return,
                    };
                    // 1. Drain control queue.
                    while let Ok(action) = s.cons.pop() {
                        match action {
                            Action::Load(p) => match s.deck.open(&p, device_rate) {
                                Ok(()) => s.deck.playing = true,
                                Err(e) => emit_err(&app, format!("load failed: {e}")),
                            },
                            Action::Play => s.deck.playing = true,
                            Action::Pause => s.deck.playing = false,
                            Action::Seek(pct) => s.deck.seek(pct, device_rate),
                            Action::SourceMode(m) => s.source_mode = m,
                        }
                    }
                    // 2. Produce + meter frames.
                    let sends = sends_for(s.source_mode);
                    for frame in out.chunks_mut(channels.max(1)) {
                        let deck_frame = s.deck.next_frame();
                        let mic_frame = s.mic_cons.pop().unwrap_or([0.0, 0.0]);
                        let mon = mix_bus(
                            &[deck_frame, mic_frame],
                            &[sends.decks_monitor, sends.mic_monitor],
                        );
                        s.meter.push(mon, mic_frame);
                        write_frame(frame, mon[0], mon[1]);
                    }
                    (s.meter.take(), s.deck.take_ended())
                };
                // 3. Emit outside the lock.
                if let Some(l) = levels {
                    if let Some(h) = app.lock().ok().and_then(|g| g.clone()) {
                        let _ = h.emit(events::LEVELS, l);
                    }
                }
                if ended {
                    if let Some(h) = app.lock().ok().and_then(|g| g.clone()) {
                        let _ = h.emit(events::TRACK_ENDED, ());
                    }
                }
            },
            move |_err: cpal::StreamError| {
                // Device lost / stream error → ask the audio loop to rebuild on
                // the current default device instead of surfacing a hard error.
                let _ = sig_tx.send(Signal::Rebuild);
            },
            None,
        )
        .map_err(|e| e.to_string())?;
    Ok(stream)
}
