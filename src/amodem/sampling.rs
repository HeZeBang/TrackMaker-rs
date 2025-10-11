pub struct Sampler<'a> {
    signal: &'a [f64],
    position: f64,
    freq: f64,
}

impl<'a> Sampler<'a> {
    pub fn new(signal: &'a [f64], freq: f64) -> Self {
        Self {
            signal,
            position: 0.0,
            freq,
        }
    }

    pub fn take(&mut self, size: usize) -> Option<Vec<f64>> {
        if size == 0 {
            return Some(Vec::new());
        }

        let mut result = Vec::with_capacity(size);
        let start_pos = self.position;

        for _ in 0..size {
            let base = self.position.floor();
            let idx = base as usize;
            if idx >= self.signal.len() {
                // return None if not enough data remaining to fill the frame
                self.position = start_pos;
                return None;
            }

            // Linear interpolation
            let frac = self.position - base;
            let sample = if frac > 0.0 && idx + 1 < self.signal.len() {
                let current = self.signal[idx];
                let next = self.signal[idx + 1];
                current + frac * (next - current)
            } else {
                self.signal[idx]
            };

            result.push(sample);
            self.position += self.freq;
        }

        Some(result)
    }

    pub fn has_data(&self) -> bool {
        (self.position as usize) < self.signal.len()
    }
}
