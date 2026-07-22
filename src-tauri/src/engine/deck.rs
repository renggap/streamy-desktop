/// A single playback voice. Holds fully decoded interleaved PCM and a read
/// position; the audio callback pulls one frame at a time via [`Deck::next_frame`].
pub struct Deck {
    pub samples: Vec<f32>, // interleaved stereo f32
    pub channels: usize,
    pub pos: usize, // sample index
    pub playing: bool,
}

impl Deck {
    pub fn new() -> Self {
        Self {
            samples: vec![],
            channels: 2,
            pos: 0,
            playing: false,
        }
    }

    pub fn load(&mut self, samples: Vec<f32>, channels: usize) {
        self.samples = samples;
        self.channels = channels;
        self.pos = 0;
    }

    /// Next stereo frame. Emits silence when stopped; up-mixes mono.
    pub fn next_frame(&mut self) -> [f32; 2] {
        if !self.playing || self.channels == 0 {
            return [0.0, 0.0];
        }
        let c = self.channels;
        let frame = match c {
            2 => [
                self.samples.get(self.pos).copied().unwrap_or(0.0),
                self.samples.get(self.pos + 1).copied().unwrap_or(0.0),
            ],
            1 => {
                let m = self.samples.get(self.pos).copied().unwrap_or(0.0);
                [m, m]
            }
            _ => [0.0, 0.0],
        };
        self.pos += c;
        if self.pos + c > self.samples.len() {
            self.playing = false;
            self.pos = 0;
        }
        frame
    }
}

impl Default for Deck {
    fn default() -> Self {
        Self::new()
    }
}
