use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{BufferSize, SampleRate, Stream, StreamConfig};
use tauri::{AppHandle, Emitter};

use super::deck::Deck;
use super::eq::Eq3Band;
use super::meter::{Meter, Position};
use super::mic::MicInput;
use super::mixer::{duck_gain, equal_power, mix_bus, sends_for, SourceMode};
use crate::events;

const FALLBACK_RATE: u32 = 44100;
/// Crossfade ramp speed: ~0.4s to cross the full sweep at the device rate.
const CROSSFADE_SECS: f32 = 0.4;
/// Mic-duck envelope smoothing factor per frame.
const DUCK_SMOOTH: f32 = 0.02;

/// Control messages pushed from the UI thread, drained at the top of each
/// audio callback.
pub enum Action {
    Load(String),
    Play,
    Pause,
    Seek(f64),
    SourceMode(SourceMode),
    SetCrossfade(f32),
    SetEq(f64, f64, f64),
    SetDuck(bool, f32, f32),
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
    deck_a: Deck,
    deck_b: Deck,
    /// 0 = deck A is "current", 1 = deck B. Playlist Load targets the other.
    active: u8,
    /// Crossfader position 0..1 (0 = full A, 1 = full B), ramped toward target.
    crossfade: f32,
    crossfade_target: f32,
    fade_step: f32,
    eq_a: Eq3Band,
    eq_b: Eq3Band,
    duck_enabled: bool,
    duck_threshold: f32,
    duck_depth_db: f32,
    music_rms: f32,
    duck_gain: f32,
    meter: Meter,
    source_mode: SourceMode,
    cons: rtrb::Consumer<Action>,
    mic_cons: rtrb::Consumer<[f32; 2]>,
    /// When broadcasting, the callback pushes the broadcast bus into this ring
    /// for the encoder thread to consume.
    broadcast_tx: Option<rtrb::Producer<[f32; 2]>>,
    /// When recording, the callback taps the same broadcast mix into this ring
    /// for the recorder thread. Independent of `broadcast_tx` (record-only works).
    record_tx: Option<rtrb::Producer<[f32; 2]>>,
}

impl EngineState {
    fn active_deck(&mut self) -> &mut Deck {
        if self.active == 0 {
            &mut self.deck_a
        } else {
            &mut self.deck_b
        }
    }
}

/// Send + `Sync` handle to the audio backend.
pub struct AudioEngine {
    control: Mutex<rtrb::Producer<Action>>,
    sig: mpsc::Sender<Signal>,
    app: Arc<Mutex<Option<AppHandle>>>,
    mic: MicInput,
    state: Arc<Mutex<EngineState>>,
    /// Chosen output device name (None = system default). Drives stream rebuilds.
    output_device: Arc<Mutex<Option<String>>>,
}

impl AudioEngine {
    pub fn new() -> Self {
        let (prod, cons) = rtrb::RingBuffer::<Action>::new(64);
        let (mic_prod, mic_cons) = rtrb::RingBuffer::<[f32; 2]>::new(8192);
        let mic_prod = Arc::new(Mutex::new(mic_prod));
        let mic = MicInput::new(mic_prod);

        let (sig_tx, sig_rx) = mpsc::channel::<Signal>();
        let app: Arc<Mutex<Option<AppHandle>>> = Arc::new(Mutex::new(None));
        let output_device: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let state = Arc::new(Mutex::new(EngineState {
            deck_a: Deck::new(),
            deck_b: Deck::new(),
            active: 0,
            crossfade: 0.0,
            crossfade_target: 0.0,
            fade_step: 1.0 / (CROSSFADE_SECS * FALLBACK_RATE as f32),
            eq_a: Eq3Band::new(FALLBACK_RATE as f64),
            eq_b: Eq3Band::new(FALLBACK_RATE as f64),
            duck_enabled: false,
            duck_threshold: 0.15,
            duck_depth_db: 12.0,
            music_rms: 0.0,
            duck_gain: 1.0,
            meter: Meter::new(FALLBACK_RATE),
            source_mode: SourceMode::File,
            cons,
            mic_cons,
            broadcast_tx: None,
            record_tx: None,
        }));

        let app_for_thread = app.clone();
        let state_for_thread = state.clone();
        let out_for_thread = output_device.clone();
        let sig_for_thread = sig_tx.clone();
        thread::spawn(move || {
            audio_loop(state_for_thread, app_for_thread, out_for_thread, sig_for_thread, sig_rx)
        });

        Self {
            control: Mutex::new(prod),
            sig: sig_tx,
            app,
            mic,
            state,
            output_device,
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
    pub fn set_crossfade(&self, pos: f32) -> Result<(), String> {
        self.push(Action::SetCrossfade(pos))
    }
    pub fn set_eq(&self, low_db: f64, mid_db: f64, high_db: f64) -> Result<(), String> {
        self.push(Action::SetEq(low_db, mid_db, high_db))
    }
    pub fn set_duck(&self, enabled: bool, threshold: f32, depth_db: f32) -> Result<(), String> {
        self.push(Action::SetDuck(enabled, threshold, depth_db))
    }
    pub fn set_input_device(&self, name: String) -> Result<(), String> {
        self.mic.set_device(name)
    }

    /// Target an output device by name (None = system default); rebuilds the stream.
    pub fn set_output_device(&self, name: Option<String>) -> Result<(), String> {
        if let Ok(mut g) = self.output_device.lock() {
            *g = name;
        }
        let _ = self.sig.send(Signal::Rebuild);
        Ok(())
    }

    /// Restore the OS default input (startup / when no saved device).
    pub fn open_default_input(&self) -> Result<(), String> {
        self.mic.open_default()
    }

    /// Install/remove the broadcast-bus producer the callback pushes to.
    pub fn set_broadcast_producer(&self, tx: Option<rtrb::Producer<[f32; 2]>>) {
        if let Ok(mut s) = self.state.lock() {
            s.broadcast_tx = tx;
        }
    }

    /// Install/remove the record-bus producer (PCM tap of the broadcast mix).
    pub fn set_record_producer(&self, tx: Option<rtrb::Producer<[f32; 2]>>) {
        if let Ok(mut s) = self.state.lock() {
            s.record_tx = tx;
        }
    }

    /// Current output device sample rate (broadcast PCM is at this rate).
    pub fn output_rate(&self) -> u32 {
        self.state
            .lock()
            .map(|s| s.meter.rate())
            .unwrap_or(FALLBACK_RATE)
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
    output_device: Arc<Mutex<Option<String>>>,
    sig_tx: mpsc::Sender<Signal>,
    sig_rx: mpsc::Receiver<Signal>,
) {
    loop {
        let wanted = output_device.lock().ok().and_then(|g| g.clone());
        let cfg = current_output_cfg(&wanted);
        // Adapt meter window + deck resample step to this device's rate.
        if let Ok(mut s) = state.lock() {
            s.meter.set_sample_rate(cfg.sample_rate.0);
            s.deck_a.set_device_rate(cfg.sample_rate.0);
            s.deck_b.set_device_rate(cfg.sample_rate.0);
            s.fade_step = 1.0 / (CROSSFADE_SECS * cfg.sample_rate.0 as f32);
        }
        match build_output_stream(state.clone(), app.clone(), sig_tx.clone(), &cfg, &wanted) {
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

/// Pick the output device by name, falling back to the system default.
fn pick_output_device(wanted: &Option<String>) -> Option<cpal::Device> {
    let host = cpal::default_host();
    if let Some(name) = wanted {
        if let Ok(mut iter) = host.output_devices() {
            if let Some(d) = iter.find(|dev| dev.name().ok().as_deref() == Some(name.as_str())) {
                return Some(d);
            }
        }
    }
    host.default_output_device()
}

fn current_output_cfg(wanted: &Option<String>) -> StreamConfig {
    match pick_output_device(wanted)
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
    wanted: &Option<String>,
) -> Result<Stream, String> {
    let device = pick_output_device(wanted)
        .ok_or_else(|| "no output device".to_string())?;
    let channels = cfg.channels as usize;
    let device_rate = cfg.sample_rate.0;

    let stream = device
        .build_output_stream(
            cfg,
            move |out: &mut [f32], _: &cpal::OutputCallbackInfo| {
                let (levels, pos, ended) = {
                    let mut s = match state.lock() {
                        Ok(g) => g,
                        Err(_) => return,
                    };
                    // 1. Drain control queue.
                    while let Ok(action) = s.cons.pop() {
                        match action {
                            // Load into the INACTIVE deck, flip active, and ramp
                            // the crossfader to it — A/B alternation on track change.
                            Action::Load(p) => {
                                let target = 1 - s.active;
                                let deck = if target == 0 { &mut s.deck_a } else { &mut s.deck_b };
                                match deck.open(&p, device_rate) {
                                    Ok(()) => {
                                        deck.playing = true;
                                        s.active = target;
                                        s.crossfade_target = if target == 0 { 0.0 } else { 1.0 };
                                    }
                                    Err(e) => emit_err(&app, format!("load failed: {e}")),
                                }
                            }
                            Action::Play => s.active_deck().playing = true,
                            Action::Pause => s.active_deck().playing = false,
                            Action::Seek(pct) => s.active_deck().seek(pct, device_rate),
                            Action::SourceMode(m) => s.source_mode = m,
                            Action::SetCrossfade(pos) => {
                                let p = pos.clamp(0.0, 1.0);
                                s.crossfade = p;
                                s.crossfade_target = p;
                            }
                            Action::SetEq(lo, mid, hi) => {
                                s.eq_a.set_gains_db(lo, mid, hi);
                                s.eq_b.set_gains_db(lo, mid, hi);
                            }
                            Action::SetDuck(en, th, db) => {
                                s.duck_enabled = en;
                                s.duck_threshold = th;
                                s.duck_depth_db = db;
                            }
                        }
                    }
                    // 2. Produce + meter frames.
                    let sends = sends_for(s.source_mode);
                    for frame in out.chunks_mut(channels.max(1)) {
                        // Two decks -> per-deck EQ -> equal-power crossfade.
                        let raw_a = s.deck_a.next_frame();
                        let raw_b = s.deck_b.next_frame();
                        let a = s.eq_a.process(raw_a);
                        let b = s.eq_b.process(raw_b);
                        let diff = s.crossfade_target - s.crossfade;
                        if diff.abs() <= s.fade_step {
                            s.crossfade = s.crossfade_target;
                        } else {
                            s.crossfade += s.fade_step * diff.signum();
                        }
                        let (ga, gb) = equal_power(s.crossfade);
                        let deck_frame = [a[0] * ga + b[0] * gb, a[1] * ga + b[1] * gb];

                        // Mic auto-duck: track music RMS, ease the duck gain.
                        let inst = ((deck_frame[0] * deck_frame[0] + deck_frame[1] * deck_frame[1]) * 0.5).sqrt();
                        s.music_rms += DUCK_SMOOTH * (inst - s.music_rms);
                        let mic_frame = s.mic_cons.pop().unwrap_or([0.0, 0.0]);
                        let target_duck = if s.duck_enabled {
                            duck_gain(s.music_rms, s.duck_threshold, s.duck_depth_db)
                        } else {
                            1.0
                        };
                        s.duck_gain += DUCK_SMOOTH * (target_duck - s.duck_gain);
                        let mic_out = [mic_frame[0] * s.duck_gain, mic_frame[1] * s.duck_gain];

                        let mon = mix_bus(
                            &[deck_frame, mic_out],
                            &[sends.decks_monitor, sends.mic_monitor],
                        );
                        let bcast = mix_bus(
                            &[deck_frame, mic_out],
                            &[sends.decks_broadcast, sends.mic_broadcast],
                        );
                        if let Some(tx) = s.broadcast_tx.as_mut() {
                            let _ = tx.push(bcast);
                        }
                        if let Some(tx) = s.record_tx.as_mut() {
                            let _ = tx.push(bcast);
                        }
                        s.meter.push(mon, mic_out);
                        write_frame(frame, mon[0], mon[1]);
                    }
                    let levels = s.meter.take();
                    let pos = levels
                        .is_some()
                        .then(|| s.active_deck().position())
                        .flatten()
                        .map(|(p, e, t)| Position {
                            pct: p,
                            elapsed_secs: e,
                            total_secs: t,
                        });
                    (levels, pos, s.active_deck().take_ended())
                };
                // 3. Emit outside the lock.
                if let Some(l) = levels {
                    if let Some(h) = app.lock().ok().and_then(|g| g.clone()) {
                        let _ = h.emit(events::LEVELS, l);
                    }
                }
                if let Some(p) = pos {
                    if let Some(h) = app.lock().ok().and_then(|g| g.clone()) {
                        let _ = h.emit(events::POSITION, p);
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
