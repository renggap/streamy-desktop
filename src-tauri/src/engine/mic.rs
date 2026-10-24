// P1.3: microphone capture. A dedicated thread owns the cpal `InputStream`
// (!Send, like the output stream) and pushes captured stereo frames into a
// drop-oldest ring whose consumer lives in the output callback. Device
// selection re-opens the stream on the mic thread and reports the open
// outcome back to the caller — a mic that fails to open (unplugged,
// exclusive-mode capture, permissions) surfaces as an error instead of a
// silently dead input. The mic device's sample rate is published via a
// shared atomic so the engine can resample into the output rate (a 48 kHz
// mic into a 44.1 kHz output must not be consumed raw).

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use super::ring::StereoRingProducer;

pub enum MicCmd {
    /// Open `name` ("" = OS default); report the outcome on `reply`.
    SetDevice {
        name: String,
        reply: mpsc::Sender<Result<(), String>>,
    },
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

    /// Open `name` ("" = OS default). Blocks (bounded) for the mic thread's
    /// open result so failures reach the caller.
    pub fn set_device(&self, name: String) -> Result<(), String> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.control
            .send(MicCmd::SetDevice {
                name,
                reply: reply_tx,
            })
            .map_err(|e| format!("mic thread unavailable: {e}"))?;
        match reply_rx.recv_timeout(std::time::Duration::from_secs(5)) {
            Ok(r) => r,
            Err(_) => Err("mic open timed out (device not responding)".into()),
        }
    }
}

impl Drop for MicInput {
    fn drop(&mut self) {
        let _ = self.control.send(MicCmd::Shutdown);
    }
}

fn mic_loop(producer: StereoRingProducer, rate: Arc<AtomicU32>, rx: mpsc::Receiver<MicCmd>) {
    let host = cpal::default_host();
    let mut stream = open_default(&host, &producer, &rate).ok();
    while let Ok(cmd) = rx.recv() {
        match cmd {
            MicCmd::SetDevice { name, reply } => {
                drop(stream.take());
                let res = if name.is_empty() {
                    open_default(&host, &producer, &rate)
                } else {
                    open_by_name(&host, &name, &producer, &rate)
                };
                let _ = reply.send(res.map(|s| {
                    stream = Some(s);
                }));
            }
            MicCmd::Shutdown => break,
        }
    }
}

fn open_default(
    host: &cpal::Host,
    producer: &StereoRingProducer,
    rate: &Arc<AtomicU32>,
) -> Result<cpal::Stream, String> {
    let dev = host
        .default_input_device()
        .ok_or_else(|| "no default input device (no microphone?)".to_string())?;
    open_on(dev, producer, rate)
}

fn open_by_name(
    host: &cpal::Host,
    name: &str,
    producer: &StereoRingProducer,
    rate: &Arc<AtomicU32>,
) -> Result<cpal::Stream, String> {
    let dev = host
        .input_devices()
        .map_err(|e| format!("cannot enumerate input devices: {e}"))?
        .find(|d| d.name().ok().as_deref() == Some(name))
        .ok_or_else(|| format!("input device not found: {name}"))?;
    open_on(dev, producer, rate)
}

fn open_on(
    dev: cpal::Device,
    producer: &StereoRingProducer,
    rate: &Arc<AtomicU32>,
) -> Result<cpal::Stream, String> {
    let name = dev.name().unwrap_or_default();
    let cfg = dev
        .default_input_config()
        .map_err(|e| format!("input '{name}' has no usable config: {e}"))?;
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
            |err| crate::diag::log(format!("mic stream error: {err}")),
            None,
        )
        .map_err(|e| format!("cannot open input '{name}': {e}"))?;
    stream
        .play()
        .map_err(|e| format!("input '{name}' refused to start: {e}"))?;
    Ok(stream)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A device name that matches nothing reports a clear error instead of
    /// silently leaving the mic dead. Deterministic on any host (headless or
    /// not): the bogus name never matches a real device.
    #[test]
    fn unknown_device_errors_instead_of_silent_nothing() {
        let (tx, _rx_keepalive) = crate::engine::ring::StereoRing::new(1024);
        let mic = MicInput::new(tx);
        let err = mic
            .set_device("definitely-not-a-real-device-xyz".into())
            .expect_err("bogus device must error");
        assert!(
            err.contains("not found"),
            "expected a not-found error, got: {err}"
        );
    }
}
