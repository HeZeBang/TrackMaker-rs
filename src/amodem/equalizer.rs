use crate::amodem::{config::Configuration, dsp::Prbs};
use num_complex::Complex64;

pub const EQUALIZER_LENGTH: usize = 200;
pub const SILENCE_LENGTH: usize = 50;

pub fn get_prefix() -> Vec<f64> {
    let mut prefix = vec![1.0; EQUALIZER_LENGTH];
    prefix.extend(vec![0.0; SILENCE_LENGTH]);
    prefix
}

pub struct Equalizer {
    carriers: Vec<Vec<Complex64>>,
    nfreq: usize,
    nsym: usize,
}

impl Equalizer {
    pub fn new(config: &Configuration) -> Self {
        Self {
            carriers: config.carriers.clone(),
            nfreq: config.nfreq,
            nsym: config.nsym,
        }
    }

    pub fn train_symbols(
        &self,
        length: usize,
        _config: &Configuration,
    ) -> Vec<Vec<Complex64>> {
        let constant_prefix = 16;
        let mut prbs = Prbs::new(1, 0x1100b, 2);
        let constellation = [
            Complex64::new(1.0, 0.0),  // 0: 1
            Complex64::new(0.0, 1.0),  // 1: 1j
            Complex64::new(-1.0, 0.0), // 2: -1
            Complex64::new(0.0, -1.0), // 3: -1j
        ];

        let mut symbols = Vec::new();
        for i in 0..length {
            let mut symbol_row = Vec::new();
            for _ in 0..self.nfreq {
                let prbs_val = prbs.next().unwrap() as usize;
                let symbol = constellation[prbs_val];
                if i < constant_prefix {
                    // Use constant symbol but still advance PRBS
                    symbol_row.push(Complex64::new(1.0, 0.0));
                } else {
                    symbol_row.push(symbol);
                }
            }
            symbols.push(symbol_row);
        }

        symbols
    }

    pub fn modulator(&self, symbols: &[Vec<Complex64>]) -> Vec<f64> {
        let gain = 1.0 / self.carriers.len() as f64;
        let mut result = Vec::new();

        for symbol_row in symbols {
            // Compute dot product: np.dot(symbol_row, self.carriers)
            let mut signal = vec![Complex64::new(0.0, 0.0); self.nsym];

            for (i, &symbol) in symbol_row.iter().enumerate() {
                if i < self.carriers.len() {
                    for (j, &carrier) in self.carriers[i]
                        .iter()
                        .enumerate()
                    {
                        signal[j] += symbol * carrier;
                    }
                }
            }

            // Convert to real signal and apply gain
            let real_signal: Vec<f64> = signal
                .iter()
                .map(|c| c.re * gain)
                .collect();

            result.extend(real_signal);
        }

        result
    }
}
