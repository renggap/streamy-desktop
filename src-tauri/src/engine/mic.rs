// P1.3: microphone capture. A dedicated thread owns the cpal `InputStream`
// (!Send, like the output stream) and pushes captured stereo frames into an
// rtrb ring whose consumer lives in the output callback. Device selection
// re-opens the stream on the mic thread.

use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rtrb::Producer;

pub enum MicCmd {
    SetDevice(String),
    Shutdown,
}

pub struct MicInput {
    control: mpsc::Sender<MicCmd>,
}

impl MicInput {
    /// Spawn the mic thread. `producer` is shared with the mic input callback.
    pub fn new(producer: Arc<Mutex<Producer<[f32; 2]>>>) -> Self {
        let (tx, rx) = mpsc::channel::<MicCmd>();
        thread::spawn(move || mic_loop(producer, rx));
        Self { control: tx }
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

fn mic_loop(producer: Arc<Mutex<Producer<[f32; 2]>>>, rx: mpsc::Receiver<MicCmd>) {
    let host = cpal::default_host();
    let mut stream = open_default(&host, &producer);
    while let Ok(cmd) = rx.recv() {
        match cmd {
            MicCmd::SetDevice(name) => {
                drop(stream.take());
                stream = if name.is_empty() {
                    open_default(&host, &producer)
                } else {
                    open_by_name(&host, &name, &producer)
                };
            }
            MicCmd::Shutdown => break,
        }
    }
}

fn open_default(host: &cpal::Host, producer: &Arc<Mutex<Producer<[f32; 2]>>>) -> Option<cpal::Stream> {
    let dev = host.default_input_device()?;
    open_on(dev, producer)
}

fn open_by_name(
    host: &cpal::Host,
    name: &str,
    producer: &Arc<Mutex<Producer<[f32; 2]>>>,
) -> Option<cpal::Stream> {
    let dev = host
        .input_devices()
        .ok()?
        .find(|d| d.name().ok().as_deref() == Some(name))?;
    open_on(dev, producer)
}

fn open_on(
    dev: cpal::Device,
    producer: &Arc<Mutex<Producer<[f32; 2]>>>,
) -> Option<cpal::Stream> {
    let cfg = dev.default_input_config().ok()?;
    let stream_cfg = cpal::StreamConfig::from(cfg);
    let channels = stream_cfg.channels as usize;
    let producer = producer.clone();
    let stream = dev
        .build_input_stream(
            &stream_cfg,
            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                let mut p = match producer.lock() {
                    Ok(g) => g,
                    Err(_) => return,
                };
                for frame in data.chunks(channels.max(1)) {
                    let l = frame.first().copied().unwrap_or(0.0);
                    let r = frame.get(1).copied().unwrap_or(l);
                    // Drop newest if the consumer (output) can't keep up.
                    let _ = p.push([l, r]);
                }
            },
            |err| eprintln!("mic stream error: {err}"),
            None,
        )
        .ok()?;
    stream.play().ok()?;
    Some(stream)
}
