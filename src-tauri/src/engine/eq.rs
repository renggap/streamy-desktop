// P5.3: 3-band EQ per deck channel. RBJ (Audio EQ Cookbook) biquads in
// Direct Form II Transposed, cascaded as low-shelf / peaking / high-shelf.
//
// Pure math; the engine callback calls Eq3Band::process on each deck frame.

/// Normalized biquad (a0 == 1). DF II Transposed for numerical stability.
#[derive(Clone, Copy)]
pub struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    z1: f64,
    z2: f64,
}

impl Biquad {
    pub fn identity() -> Self {
        Self {
            b0: 1.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
            z1: 0.0,
            z2: 0.0,
        }
    }

    /// Build from raw RBJ cookbook coeffs (a0..a2, b0..b2), normalized to a0=1.
    fn from_coeffs(b0: f64, b1: f64, b2: f64, a0: f64, a1: f64, a2: f64) -> Self {
        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
            z1: 0.0,
            z2: 0.0,
        }
    }

    /// Peaking filter: `gain_db` at `freq`, bandwidth set by `q`.
    pub fn peaking(sample_rate: f64, freq: f64, q: f64, gain_db: f64) -> Self {
        let a = 10.0_f64.powf(gain_db / 40.0);
        let w0 = 2.0 * std::f64::consts::PI * freq / sample_rate;
        let (cw, sw) = (w0.cos(), w0.sin());
        let alpha = sw / (2.0 * q);
        Self::from_coeffs(
            1.0 + alpha * a,
            -2.0 * cw,
            1.0 - alpha * a,
            1.0 + alpha / a,
            -2.0 * cw,
            1.0 - alpha / a,
        )
    }

    /// Low-shelf: boost/cut below `freq` by `gain_db`.
    pub fn low_shelf(sample_rate: f64, freq: f64, gain_db: f64) -> Self {
        let (a, cw, sw, q) = shelf_params(sample_rate, freq, gain_db);
        let alpha = sw / 2.0 * ((a + 1.0 / a) * (1.0 / q - 1.0) + 2.0).sqrt();
        Self::from_coeffs(
            a * ((a + 1.0) - (a - 1.0) * cw + 2.0 * a.sqrt() * alpha),
            2.0 * a * ((a - 1.0) - (a + 1.0) * cw),
            a * ((a + 1.0) - (a - 1.0) * cw - 2.0 * a.sqrt() * alpha),
            (a + 1.0) + (a - 1.0) * cw + 2.0 * a.sqrt() * alpha,
            -2.0 * ((a - 1.0) + (a + 1.0) * cw),
            (a + 1.0) + (a - 1.0) * cw - 2.0 * a.sqrt() * alpha,
        )
    }

    /// High-shelf: boost/cut above `freq` by `gain_db`.
    pub fn high_shelf(sample_rate: f64, freq: f64, gain_db: f64) -> Self {
        let (a, cw, sw, q) = shelf_params(sample_rate, freq, gain_db);
        let alpha = sw / 2.0 * ((a + 1.0 / a) * (1.0 / q - 1.0) + 2.0).sqrt();
        Self::from_coeffs(
            a * ((a + 1.0) + (a - 1.0) * cw + 2.0 * a.sqrt() * alpha),
            -2.0 * a * ((a - 1.0) + (a + 1.0) * cw),
            a * ((a + 1.0) + (a - 1.0) * cw - 2.0 * a.sqrt() * alpha),
            (a + 1.0) - (a - 1.0) * cw + 2.0 * a.sqrt() * alpha,
            2.0 * ((a - 1.0) - (a + 1.0) * cw),
            (a + 1.0) - (a - 1.0) * cw - 2.0 * a.sqrt() * alpha,
        )
    }

    /// Linear magnitude |H(e^jw)| at `freq` (for testing frequency response).
    pub fn magnitude_at(&self, freq: f64, sample_rate: f64) -> f64 {
        let w = 2.0 * std::f64::consts::PI * freq / sample_rate;
        let (cw, sw) = (w.cos(), w.sin());
        let (c2, s2) = ((2.0 * w).cos(), (2.0 * w).sin());
        let num_re = self.b0 + self.b1 * cw + self.b2 * c2;
        let num_im = -(self.b1 * sw + self.b2 * s2);
        let den_re = 1.0 + self.a1 * cw + self.a2 * c2;
        let den_im = -(self.a1 * sw + self.a2 * s2);
        ((num_re * num_re + num_im * num_im) / (den_re * den_re + den_im * den_im)).sqrt()
    }

    /// Process one sample (DF II Transposed), keeping state.
    pub fn process(&mut self, x: f32) -> f32 {
        let x = x as f64;
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y as f32
    }
}

fn shelf_params(sample_rate: f64, freq: f64, gain_db: f64) -> (f64, f64, f64, f64) {
    let a = 10.0_f64.powf(gain_db / 40.0);
    let w0 = 2.0 * std::f64::consts::PI * freq / sample_rate;
    (a, w0.cos(), w0.sin(), 0.7071067811865476) // shelf slope ~0.5 (Butterworth Q)
}

/// 3-band EQ: low shelf (bass), peaking (mid), high shelf (treble).
/// Gains in dB, 0 = flat.
pub struct Eq3Band {
    sample_rate: f64,
    low: Biquad,
    mid: Biquad,
    high: Biquad,
    gains_db: [f64; 3],
}

impl Eq3Band {
    const LOW_FREQ: f64 = 200.0;
    const MID_FREQ: f64 = 1000.0;
    const HIGH_FREQ: f64 = 5000.0;
    const MID_Q: f64 = 0.7;

    pub fn new(sample_rate: f64) -> Self {
        Self {
            sample_rate,
            low: Biquad::identity(),
            mid: Biquad::identity(),
            high: Biquad::identity(),
            gains_db: [0.0; 3],
        }
    }

    /// Set (low, mid, high) gains in dB; rebuilds the biquads (keeps state).
    pub fn set_gains_db(&mut self, low_db: f64, mid_db: f64, high_db: f64) {
        self.gains_db = [low_db, mid_db, high_db];
        let sr = self.sample_rate;
        self.low = Biquad::low_shelf(sr, Self::LOW_FREQ, low_db);
        self.mid = Biquad::peaking(sr, Self::MID_FREQ, Self::MID_Q, mid_db);
        self.high = Biquad::high_shelf(sr, Self::HIGH_FREQ, high_db);
    }

    pub fn gains_db(&self) -> [f64; 3] {
        self.gains_db
    }

    /// Process a stereo frame through low -> mid -> high.
    pub fn process(&mut self, frame: [f32; 2]) -> [f32; 2] {
        let l = self.low.process(frame[0]);
        let r = self.low.process(frame[1]);
        let l = self.mid.process(l);
        let r = self.mid.process(r);
        let l = self.high.process(l);
        let r = self.high.process(r);
        [l, r]
    }

    /// Magnitude (linear) summed across the three bands at `freq` (for tests).
    pub fn magnitude_at(&self, freq: f64) -> f64 {
        self.low.magnitude_at(freq, self.sample_rate)
            * self.mid.magnitude_at(freq, self.sample_rate)
            * self.high.magnitude_at(freq, self.sample_rate)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f64 = 44100.0;

    fn db(gain_linear: f64) -> f64 {
        20.0 * gain_linear.log10()
    }

    #[test]
    fn identity_is_flat() {
        let eq = Eq3Band::new(SR);
        // flat across the band centers
        for f in [60.0, 200.0, 1000.0, 5000.0, 12000.0] {
            assert!((eq.magnitude_at(f) - 1.0).abs() < 1e-6, "not flat at {f}");
        }
    }

    #[test]
    fn mid_boost_lifts_1k() {
        let mut eq = Eq3Band::new(SR);
        eq.set_gains_db(0.0, 12.0, 0.0);
        let gain_db = db(eq.magnitude_at(1000.0));
        assert!(gain_db > 10.0, "mid boost too small: {gain_db:.2} dB");
        // and leaves the extremes mostly alone
        let low = db(eq.magnitude_at(60.0));
        let high = db(eq.magnitude_at(12000.0));
        assert!(low < 3.0, "mid boost leaked into low: {low:.2}");
        assert!(high < 3.0, "mid boost leaked into high: {high:.2}");
    }

    #[test]
    fn low_shelf_lifts_bass_only() {
        let mut eq = Eq3Band::new(SR);
        eq.set_gains_db(12.0, 0.0, 0.0);
        assert!(db(eq.magnitude_at(60.0)) > 9.0, "bass not boosted");
        assert!(db(eq.magnitude_at(5000.0)).abs() < 1.0, "treble moved by bass boost");
    }

    #[test]
    fn high_shelf_lifts_treble_only() {
        let mut eq = Eq3Band::new(SR);
        eq.set_gains_db(0.0, 0.0, 12.0);
        assert!(db(eq.magnitude_at(12000.0)) > 9.0, "treble not boosted");
        assert!(db(eq.magnitude_at(60.0)).abs() < 1.0, "bass moved by treble boost");
    }

    #[test]
    fn process_passes_dc_through_identity() {
        let mut eq = Eq3Band::new(SR);
        // identity chain: output == input
        let out = eq.process([0.5, -0.5]);
        assert!((out[0] - 0.5).abs() < 1e-6);
        assert!((out[1] + 0.5).abs() < 1e-6);
    }
}
