pub struct Sampler {
    signal: Vec<f64>,
    position: f64,
    freq: f64,
}

impl Sampler {
    pub fn new(signal: Vec<f64>, freq: f64) -> Self {
        Self {
            signal,
            position: 0.0,
            freq,
        }
    }
    
    pub fn take(&mut self, size: usize) -> Vec<f64> {
        let mut result = Vec::new();
        
        for _ in 0..size {
            let index = self.position as usize;
            if index < self.signal.len() {
                result.push(self.signal[index]);
                self.position += self.freq;
            } else {
                result.push(0.0); // Padding with zeros
            }
        }
        
        result
    }
    
    pub fn has_data(&self) -> bool {
        (self.position as usize) < self.signal.len()
    }
}
