use std::sync::atomic::{AtomicU32, Ordering};
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
use super::ring::StereoRingProducer;
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
    SetVolume(f32),
    SetMonitorFileGain(f32),
}

/// Signals to the audio loop: rebuild the stream (e.g. output device lost) or
/// shut down.
enum Signal {
    Rebuild,
    Shutdown,
}

// LOCKING DISCIPLINE (audio callback safety):
// - The output callback holds `state` for the whole buffer. Event emission
//   normally happens after the guard drops (levels/position/ended), with ONE
//   documented exception: a failed Action::Load emits an error event while the
//   lock is still held (rare, non-realtime path).
// - `state` is mutated from outside the callback only by O(1) field swaps:
//   set_broadcast_producer / set_record_producer / output_rate and the
//   audio_loop rate updates. The per-frame DSP loop does no I/O, no
//   allocation, and no network.
// - Action::Load is the one I/O exception: deck.open probes the file
//   (format sniff + metadata) under the lock. It is a rare user action, not
//   per-buffer work.
// - parking_lot::Mutex is non-poisoning: a panic in the callback releases the
//   lock and the next command succeeds instead of wedging the engine.
// - mic input is a separate ring; its producer mutex is never held while
//   pushing the output stream.

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
    /// Master output gain (1.0 = unity). Applied to the monitor + broadcast mix.
    volume: f32,
    /// DJ's monitor-only gain for local file playback; broadcast NOT affected.
    /// 1.0 = unity.
    monitor_file_gain: f32,
    meter: Meter,
    source_mode: SourceMode,
    cons: rtrb::Consumer<Action>,
    /// True once the audio loop opened an output stream (no-device guard).
    has_output: bool,
    mic_cons: crate::engine::ring::StereoRingConsumer,
    /// Mic device sample rate, published by the mic thread (see `mic.rs`).
    mic_rate: Arc<AtomicU32>,
    /// Output device sample rate, set on stream (re)open in `start()`.
    out_rate: Arc<AtomicU32>,
    /// Fractional read position into the mic stream, advanced by
    /// `mic_rate / output_rate` per output frame (linear-interp resample,
    /// same approach as the decks). Fixes pitch/throughput corruption when
    /// the mic device and output device run at different rates.
    mic_pos: f64,
    /// Cached mic frames for interpolation (last popped pair).
    mic_prev: [f32; 2],
    mic_next: [f32; 2],
    /// When broadcasting, the callback pushes the broadcast bus into this ring
    /// for the encoder thread to consume. Drop-oldest overflow (design §7):
    /// a slow uplink makes the audience skip forward, not freeze.
    broadcast_tx: Option<StereoRingProducer>,
    /// When recording, the callback taps the same broadcast mix into this ring
    /// for the recorder thread. Independent of `broadcast_tx` (record-only works).
    record_tx: Option<StereoRingProducer>,
}

impl EngineState {
    fn active_deck(&mut self) -> &mut Deck {
        if self.active == 0 {
            &mut self.deck_a
        } else {
            &mut self.deck_b
        }
    }

    /// Render one output buffer: decks → EQ → crossfade → duck → monitor/broadcast
    /// buses → write to `out`. Pushes to broadcast_tx / record_tx / meter.
    /// Pure DSP; no events, no locks beyond caller-held.
    ///
    /// `channels` is the output device channel count (e.g. 2 for stereo).
    /// `out` is partitioned into frames of `channels` samples; `write_frame`
    /// handles mono/2ch/Nch expansion per frame.
    fn render_buffer(&mut self, out: &mut [f32], channels: usize) {
        let sends = sends_for(self.source_mode);
        for frame in out.chunks_mut(channels.max(1)) {
            // Two decks -> per-deck EQ -> equal-power crossfade.
            let raw_a = self.deck_a.next_frame();
            let raw_b = self.deck_b.next_frame();
            let a = self.eq_a.process(raw_a);
            let b = self.eq_b.process(raw_b);
            let diff = self.crossfade_target - self.crossfade;
            if diff.abs() <= self.fade_step {
                self.crossfade = self.crossfade_target;
            } else {
                self.crossfade += self.fade_step * diff.signum();
            }
            let (ga, gb) = equal_power(self.crossfade);
            let deck_frame = [a[0] * ga + b[0] * gb, a[1] * ga + b[1] * gb];

            // Mic auto-duck: track music RMS, ease the duck gain.
            let inst =
                ((deck_frame[0] * deck_frame[0] + deck_frame[1] * deck_frame[1]) * 0.5).sqrt();
            self.music_rms += DUCK_SMOOTH * (inst - self.music_rms);
            // Resample mic → output rate. mic_rate is published by the mic
            // thread (AtomicU32); step = mic_rate / out_rate. Linear interp
            // between mic_prev and mic_next at fractional position mic_pos.
            // ponytail: linear interp; switch to a real polyphase filter if
            // announcer voices ever measure aliasy.
            let step = self.mic_rate.load(Ordering::Relaxed) as f64
                / self.out_rate.load(Ordering::Relaxed).max(1) as f64;
            let t = self.mic_pos as f32;
            let mic_frame = [
                self.mic_prev[0] + (self.mic_next[0] - self.mic_prev[0]) * t,
                self.mic_prev[1] + (self.mic_next[1] - self.mic_prev[1]) * t,
            ];
            self.mic_pos += step;
            while self.mic_pos >= 1.0 {
                self.mic_pos -= 1.0;
                self.mic_prev = self.mic_next;
                self.mic_next = self.mic_cons.pop().unwrap_or(self.mic_next);
            }
            let target_duck = if self.duck_enabled {
                duck_gain(self.music_rms, self.duck_threshold, self.duck_depth_db)
            } else {
                1.0
            };
            self.duck_gain += DUCK_SMOOTH * (target_duck - self.duck_gain);
            let mic_out = [mic_frame[0] * self.duck_gain, mic_frame[1] * self.duck_gain];

            let mon = mix_bus(
                &[
                    [
                        deck_frame[0] * self.monitor_file_gain,
                        deck_frame[1] * self.monitor_file_gain,
                    ],
                    mic_out,
                ],
                &[sends.decks_monitor, sends.mic_monitor],
            );
            let bcast = mix_bus(
                &[deck_frame, mic_out],
                &[sends.decks_broadcast, sends.mic_broadcast],
            );
            // Master gain on the final mix (monitor + broadcast alike).
            let g = self.volume;
            let mon = [mon[0] * g, mon[1] * g];
            let bcast = [bcast[0] * g, bcast[1] * g];
            if let Some(tx) = self.broadcast_tx.as_ref() {
                tx.push(bcast);
            }
            if let Some(tx) = self.record_tx.as_ref() {
                tx.push(bcast);
            }
            self.meter.push(mon, mic_out);
            write_frame(frame, mon[0], mon[1]);
        }
    }
}

/// Send + `Sync` handle to the audio backend.
pub struct AudioEngine {
    control: Mutex<rtrb::Producer<Action>>,
    sig: mpsc::Sender<Signal>,
    app: Arc<Mutex<Option<AppHandle>>>,
    mic: MicInput,
    state: Arc<parking_lot::Mutex<EngineState>>,
    /// Chosen output device name (None = system default). Drives stream rebuilds.
    output_device: Arc<Mutex<Option<String>>>,
}

impl AudioEngine {
    pub fn new() -> Self {
        let (prod, cons) = rtrb::RingBuffer::<Action>::new(64);
        let (mic_prod, mic_cons) = super::ring::StereoRing::new(8192);
        let mic = MicInput::new(mic_prod.clone());
        let mic_rate = mic.rate.clone();
        let out_rate = Arc::new(AtomicU32::new(FALLBACK_RATE));

        let (sig_tx, sig_rx) = mpsc::channel::<Signal>();
        let app: Arc<Mutex<Option<AppHandle>>> = Arc::new(Mutex::new(None));
        let output_device: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let state = Arc::new(parking_lot::Mutex::new(EngineState {
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
            volume: 1.0,
            monitor_file_gain: 1.0,
            meter: Meter::new(FALLBACK_RATE),
            source_mode: SourceMode::File,
            has_output: false,
            cons,
            mic_cons,
            mic_rate,
            out_rate: out_rate.clone(),
            mic_pos: 0.0,
            mic_prev: [0.0, 0.0],
            mic_next: [0.0, 0.0],
            broadcast_tx: None,
            record_tx: None,
        }));

        let app_for_thread = app.clone();
        let state_for_thread = state.clone();
        let out_for_thread = output_device.clone();
        let sig_for_thread = sig_tx.clone();
        thread::spawn(move || {
            audio_loop(
                state_for_thread,
                app_for_thread,
                out_for_thread,
                sig_for_thread,
                sig_rx,
            )
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
        p.push(a)
            .map_err(|_| "audio control queue full".to_string())
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
    /// Master output gain, 0.0..=2.0 (1.0 = unity / 100%).
    pub fn set_volume(&self, gain: f32) -> Result<(), String> {
        self.push(Action::SetVolume(gain.clamp(0.0, 2.0)))
    }
    /// Monitor-only gain for local file playback, 0.0..=2.0 (1.0 = unity).
    /// Broadcast is unaffected.
    pub fn set_monitor_file_gain(&self, gain: f32) -> Result<(), String> {
        self.push(Action::SetMonitorFileGain(gain.clamp(0.0, 2.0)))
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
    pub fn set_broadcast_producer(&self, tx: Option<StereoRingProducer>) {
        let mut s = self.state.lock();
        s.broadcast_tx = tx;
    }

    /// Install/remove the record-bus producer (PCM tap of the broadcast mix).
    pub fn set_record_producer(&self, tx: Option<StereoRingProducer>) {
        let mut s = self.state.lock();
        s.record_tx = tx;
    }

    /// Current output device sample rate (broadcast PCM is at this rate).
    pub fn output_rate(&self) -> u32 {
        self.state.lock().meter.rate()
    }

    /// True once the audio loop has actually opened an output stream.
    /// Guards connect/record against starting on a host with no audio
    /// device (the retry loop keeps trying; broadcast would encode silence).
    pub fn has_output_device(&self) -> bool {
        self.state.lock().has_output
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
    state: Arc<parking_lot::Mutex<EngineState>>,
    app: Arc<Mutex<Option<AppHandle>>>,
    output_device: Arc<Mutex<Option<String>>>,
    sig_tx: mpsc::Sender<Signal>,
    sig_rx: mpsc::Receiver<Signal>,
) {
    loop {
        let wanted = output_device.lock().ok().and_then(|g| g.clone());
        let cfg = current_output_cfg(&wanted);
        // Adapt meter window + deck resample step to this device's rate.
        {
            let mut s = state.lock();
            s.meter.set_sample_rate(cfg.sample_rate.0);
            s.deck_a.set_device_rate(cfg.sample_rate.0);
            s.deck_b.set_device_rate(cfg.sample_rate.0);
            s.fade_step = 1.0 / (CROSSFADE_SECS * cfg.sample_rate.0 as f32);
            s.out_rate.store(cfg.sample_rate.0, Ordering::Relaxed);
        }
        match build_output_stream(state.clone(), app.clone(), sig_tx.clone(), &cfg, &wanted) {
            Ok(stream) => {
                let _ = stream.play();
                state.lock().has_output = true;
                // Hold the stream until a rebuild (device lost) or shutdown.
                match sig_rx.recv() {
                    Ok(Signal::Rebuild) => continue, // drop stream, rebuild on new default
                    _ => break,
                }
            }
            Err(e) => {
                // ponytail: emit once per distinct message; a headless host
                // retries every 1s forever and would spam the UI event bus.
                thread_local! {
                    static LAST: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
                }
                let msg = format!("audio start failed: {e}");
                LAST.with(|l| {
                    let mut l = l.borrow_mut();
                    if *l != msg {
                        *l = msg.clone();
                        emit_err(&app, msg);
                    }
                });
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
    match pick_output_device(wanted).and_then(|d| d.default_output_config().ok()) {
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
    state: Arc<parking_lot::Mutex<EngineState>>,
    app: Arc<Mutex<Option<AppHandle>>>,
    sig_tx: mpsc::Sender<Signal>,
    cfg: &StreamConfig,
    wanted: &Option<String>,
) -> Result<Stream, String> {
    let device = pick_output_device(wanted).ok_or_else(|| "no output device".to_string())?;
    let channels = cfg.channels as usize;
    let device_rate = cfg.sample_rate.0;

    let stream = device
        .build_output_stream(
            cfg,
            move |out: &mut [f32], _: &cpal::OutputCallbackInfo| {
                let (levels, pos, ended) = {
                    let mut s = state.lock();
                    // 1. Drain control queue.
                    while let Ok(action) = s.cons.pop() {
                        match action {
                            // Load into the INACTIVE deck, flip active, and ramp
                            // the crossfader to it — A/B alternation on track change.
                            Action::Load(p) => {
                                let target = 1 - s.active;
                                let deck = if target == 0 {
                                    &mut s.deck_a
                                } else {
                                    &mut s.deck_b
                                };
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
                            Action::SetVolume(v) => s.volume = v.clamp(0.0, 2.0),
                            Action::SetMonitorFileGain(v) => {
                                s.monitor_file_gain = v.clamp(0.0, 2.0)
                            }
                        }
                    }
                    // 2. Render the output buffer: per-frame DSP loop.
                    s.render_buffer(out, channels);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> &'static str {
        concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/tone-60s.mp3")
    }

    fn state_with_rings() -> (
        EngineState,
        crate::engine::ring::StereoRingConsumer,
        crate::engine::ring::StereoRingConsumer,
    ) {
        let mut s = EngineState {
            deck_a: Deck::new(),
            deck_b: Deck::new(),
            active: 0,
            crossfade: 0.0,
            crossfade_target: 0.0,
            fade_step: 1.0 / (0.4 * 44100.0),
            eq_a: Eq3Band::new(44100.0),
            eq_b: Eq3Band::new(44100.0),
            duck_enabled: false,
            duck_threshold: 0.15,
            duck_depth_db: 12.0,
            music_rms: 0.0,
            duck_gain: 1.0,
            volume: 1.0,
            monitor_file_gain: 1.0,
            meter: Meter::new(44100),
            source_mode: SourceMode::Both,
            has_output: false,
            cons: rtrb::RingBuffer::new(8).1,
            mic_cons: crate::engine::ring::StereoRing::new(8).1,
            mic_rate: Arc::new(AtomicU32::new(44_100)),
            out_rate: Arc::new(AtomicU32::new(44_100)),
            mic_pos: 0.0,
            mic_prev: [0.0, 0.0],
            mic_next: [0.0, 0.0],
            broadcast_tx: None,
            record_tx: None,
        };
        s.deck_a.open(fixture(), 44100).unwrap();
        s.deck_a.playing = true;
        let (btx, brx) = crate::engine::ring::StereoRing::new(8192);
        let (rtx, rrx) = crate::engine::ring::StereoRing::new(8192);
        s.broadcast_tx = Some(btx);
        s.record_tx = Some(rtx);
        (s, brx, rrx)
    }

    #[test]
    fn mic_resampler_upsample_ratio_drains_slower() {
        // 8 kHz mic into 48 kHz out: 6x upsample -> output consumes mic
        // frames at 8000/48000 per output frame.
        let (mut s, _brx, _rrx) = state_with_rings();
        let (mtx, mr) = crate::engine::ring::StereoRing::new(65536);
        for _ in 0..60_000 {
            mtx.push([0.5, 0.5]);
        }
        s.mic_cons = mr;
        s.mic_rate.store(8_000, Ordering::Relaxed);
        s.out_rate.store(48_000, Ordering::Relaxed);
        s.source_mode = SourceMode::Mic;
        let n = 48_000usize;
        let mut buf = vec![0.0f32; n * 2];
        s.render_buffer(&mut buf, 2);
        let drained = 60_000 - s.mic_cons.len();
        let expect = (n as f64 * 8_000.0 / 48_000.0) as usize;
        assert!(
            (drained as isize - expect as isize).abs() <= 16,
            "drained {drained}, expected ~{expect}"
        );
    }

    #[test]
    fn mic_resampler_same_rate_is_passthrough_level() {
        // Equal rates: step=1, interp at integer positions -> output frame
        // equals input frame (DC level preserved exactly).
        let (mut s, _brx, _rrx) = state_with_rings();
        let (mtx, mr) = crate::engine::ring::StereoRing::new(8192);
        for _ in 0..4_096 {
            mtx.push([0.25, -0.25]);
        }
        s.mic_cons = mr;
        s.mic_rate.store(44_100, Ordering::Relaxed);
        s.out_rate.store(44_100, Ordering::Relaxed);
        s.source_mode = SourceMode::Mic;
        s.volume = 1.0;
        s.monitor_file_gain = 0.0; // isolate the mic path from the deck tone
        let mut buf = vec![0.0f32; 1_024 * 2];
        s.render_buffer(&mut buf, 2);
        // Warmup: frame0 emits the zero window; frame1 interpolates 0->0.25
        // (t=0 after the first advance, so also ~0). Steady state from
        // frame2 on: exact DC passthrough.
        for (i, v) in buf.iter().enumerate().skip(4) {
            let want = if i % 2 == 0 { 0.25 } else { -0.25 };
            assert!(
                (v - want).abs() < 1e-6,
                "frame {i}: got {v}, want {want} (passthrough corrupted)"
            );
        }
    }

    #[test]
    fn mic_resampler_starvation_holds_last_frame_not_garbage() {
        // Ring empties mid-buffer: the resampler must hold the last known
        // frame (unwrap_or(self.mic_next)), never jump to zeros/glitch.
        let (mut s, _brx, _rrx) = state_with_rings();
        let (mtx, mr) = crate::engine::ring::StereoRing::new(8192);
        mtx.push([0.7, 0.7]); // one frame only
        s.mic_cons = mr;
        s.mic_rate.store(44_100, Ordering::Relaxed);
        s.out_rate.store(44_100, Ordering::Relaxed);
        s.source_mode = SourceMode::Mic;
        s.volume = 1.0;
        s.monitor_file_gain = 0.0;
        let mut buf = vec![0.0f32; 64 * 2];
        s.render_buffer(&mut buf, 2);
        // Monitor bus gain for Mic mode: after the 2-frame warmup, a starved
        // mic must HOLD the last frame (no drop to silence, no glitch).
        assert!(
            buf.iter().skip(4).all(|v| *v > 0.05),
            "starved mic dropped to silence mid-buffer: {:?}",
            &buf[..8]
        );
    }

    #[test]
    fn mic_resampler_consumes_at_output_rate() {
        // 48 kHz mic into 44.1 kHz out: after N output frames, the mic ring
        // should have drained ~N * 44100/48000 frames (not N — the pre-fix
        // behavior consumed one mic frame per output frame, i.e. pitch-up +
        // eventual starvation).
        let (mut s, _brx, _rrx) = state_with_rings();
        let (mtx, mr) = crate::engine::ring::StereoRing::new(65536);
        for _ in 0..60_000 {
            mtx.push([0.5, 0.5]);
        }
        s.mic_cons = mr;
        s.mic_rate.store(48_000, Ordering::Relaxed);
        s.out_rate.store(44_100, Ordering::Relaxed);
        s.source_mode = SourceMode::Mic; // silence decks, hear only mic path
        let n = 48_000usize;
        let mut buf = vec![0.0f32; n * 2];
        s.render_buffer(&mut buf, 2);
        let drained = 60_000 - s.mic_cons.len();
        // Consume mic_rate/out_rate mic frames per output frame (48k source
        // into 44.1k output skips source samples; 1:1 would pitch it up).
        let expect = (n as f64 * 48_000.0 / 44_100.0) as usize;
        assert!(
            (drained as isize - expect as isize).abs() <= 64,
            "drained {drained}, expected ~{expect} (ratio consumed, not 1:1)"
        );
    }

    #[test]
    fn render_pushes_tone_to_broadcast_and_record_rings() {
        let (mut s, brx, rrx) = state_with_rings();
        let mut buf = vec![0.0f32; 8192];
        s.render_buffer(&mut buf, 2);
        let (mut bmax, mut rmax) = (0.0f32, 0.0f32);
        while let Some(f) = brx.pop() {
            bmax = bmax.max(f[0].abs()).max(f[1].abs());
        }
        while let Some(f) = rrx.pop() {
            rmax = rmax.max(f[0].abs()).max(f[1].abs());
        }
        assert!(bmax > 0.1, "broadcast ring silent, max={bmax}");
        assert!(rmax > 0.1, "record ring silent, max={rmax}");
    }

    #[test]
    fn file_mode_mutes_mic_on_broadcast_but_keeps_monitor() {
        let (mut s, brx, _rrx) = state_with_rings();
        s.source_mode = SourceMode::File;
        // Put a loud frame in the mic ring.
        let (mtx, mr) = crate::engine::ring::StereoRing::new(8);
        for _ in 0..8 {
            mtx.push([0.9, 0.9]);
        }
        s.mic_cons = mr;
        let mut buf = vec![0.0f32; 8192];
        s.render_buffer(&mut buf, 2);
        let mut bmax = 0.0f32;
        while let Some(f) = brx.pop() {
            bmax = bmax.max(f[0].abs()).max(f[1].abs());
        }
        // Broadcast carries only the tone deck (~0.9 peak at its loudest), not
        // mic + tone summed. The <= 0.95 bound depends on the fixture being
        // below 0 dBFS (safe for the sine-tone fixture).
        assert!(
            bmax <= 0.95 && bmax > 0.1,
            "broadcast should be tone-only, max={bmax}"
        );
    }

    #[test]
    fn monitor_file_gain_scales_monitor_only() {
        let (mut s, brx, _rrx) = state_with_rings();
        // Both source modes send the deck to both buses at unity; file gain must
        // only affect the monitor (out buffer), not broadcast (ring).
        s.monitor_file_gain = 0.5;
        let mut buf = vec![0.0f32; 8192];
        s.render_buffer(&mut buf, 2);
        // Monitor (buf) is halved vs. the same render at unity.
        let mut out_max = 0.0f32;
        for &v in &buf {
            out_max = out_max.max(v.abs());
        }
        let mut bmax = 0.0f32;
        while let Some(f) = brx.pop() {
            bmax = bmax.max(f[0].abs()).max(f[1].abs());
        }
        // The fixture is a ~0.62 peak tone. At gain 0.5 monitor should be
        // roughly half of broadcast, which stays at the fixture's unity level.
        assert!(
            (bmax * 0.4..=bmax * 0.6).contains(&out_max),
            "monitor should be roughly half of broadcast: out_max={out_max}, bmax={bmax}"
        );
        assert!(
            bmax > 0.5,
            "broadcast must carry significant signal, bmax={bmax}"
        );
    }
}
