// P1.3: microphone capture. A dedicated thread owns the cpal `InputStream`
// (!Send, like the output stream) and pushes captured stereo frames into a
// drop-oldest ring whose consumer lives in the output callback. Device
// selection re-opens the stream on the mic thread. The mic device's sample
// rate is published via a shared atomic so the engine can resample into the
// output rate (a 48 kHz mic into a 44.1 kHz output must not be consumed raw).

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use super::ring::StereoRingProducer;

pub enum MicCmd {
    SetDevice(String),
    Shutdown,
}

pub struct MicInput {
    control: mpsc::Sender<MicCmd>,
    /// Sample rate of the currently open mic stream (updated by the mic
    /// thread on every open; read by the audio callback for resampling).
    pub rate: Arc<AtomicU32>,
}

impl MicInput {
    /// Spawn the mic thread writing into `producer`.
    pub fn new(producer: StereoRingProducer) -> Self {
        let (tx, rx) = mpsc::channel::<MicCmd>();
        let rate = Arc::new(AtomicU32::new(44_100));
        let rate_for_thread = rate.clone();
        thread::spawn(move || mic_loop(producer, rate_for_thread, rx));
        Self { control: tx, rate }
    }

    pub fn set_device(&self, name: String) -> Result<(), String> {
        self.control
            .send(MicCmd::SetDevice(name))
            .map_err(|e| e.to_string())
    }

    /// Ask the mic thread to reopen on whatever the OS default input is.
    pub fn open_default(&self) -> Result<(), String> {
        self.control
            .send(MicCmd::SetDevice(String::new()))
            .map_err(|e| e.to_string())
    }
}

impl Drop for MicInput {
    fn drop(&mut self) {
        let _ = self.control.send(MicCmd::Shutdown);
    }
}

fn mic_loop(producer: StereoRingProducer, rate: Arc<AtomicU32>, rx: mpsc::Receiver<MicCmd>) {
    let host = cpal::default_host();
    let mut stream = open_default(&host, &producer, &rate);
    while let Ok(cmd) = rx.recv() {
        match cmd {
            MicCmd::SetDevice(name) => {
                drop(stream.take());
                stream = if name.is_empty() {
                    open_default(&host, &producer, &rate)
                } else {
                    open_by_name(&host, &name, &producer, &rate)
                };
            }
            MicCmd::Shutdown => break,
        }
    }
}

fn open_default(
    host: &cpal::Host,
    producer: &StereoRingProducer,
    rate: &Arc<AtomicU32>,
) -> Option<cpal::Stream> {
    let dev = host.default_input_device()?;
    open_on(dev, producer, rate)
}

fn open_by_name(
    host: &cpal::Host,
    name: &str,
    producer: &StereoRingProducer,
    rate: &Arc<AtomicU32>,
) -> Option<cpal::Stream> {
    let dev = host
        .input_devices()
        .ok()?
        .find(|d| d.name().ok().as_deref() == Some(name))?;
    open_on(dev, producer, rate)
}

fn open_on(
    dev: cpal::Device,
    producer: &StereoRingProducer,
    rate: &Arc<AtomicU32>,
) -> Option<cpal::Stream> {
    let cfg = dev.default_input_config().ok()?;
    let stream_cfg = cpal::StreamConfig::from(cfg);
    let channels = stream_cfg.channels as usize;
    rate.store(stream_cfg.sample_rate.0, Ordering::Relaxed);
    let producer = producer.clone();
    let stream = dev
        .build_input_stream(
            &stream_cfg,
            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                for frame in data.chunks(channels.max(1)) {
                    let l = frame.first().copied().unwrap_or(0.0);
                    let r = frame.get(1).copied().unwrap_or(l);
                    // Drop-oldest overflow: if the output side stalls, the
                    // mic keeps realtime (oldest frames skipped, not frozen).
                    producer.push([l, r]);
                }
            },
            |err| eprintln!("mic stream error: {err}"),
            None,
        )
        .ok()?;
    stream.play().ok()?;
    Some(stream)
}
