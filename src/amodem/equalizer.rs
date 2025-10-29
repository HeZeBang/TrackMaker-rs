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

/// Levinson-Durbin solver for computing FIR filter coefficients
pub fn train(
    signal: &[f64],
    expected: &[f64],
    order: usize,
    lookahead: usize,
) -> Vec<f64> {
    let n = order + lookahead;

    // Compute autocorrelation of signal
    let mut rxx = vec![0.0; n];
    for i in 0..n {
        for j in 0..(signal.len().saturating_sub(i)) {
            rxx[i] += signal[j] * signal[j + i];
        }
    }

    // Compute cross-correlation between signal and expected
    let mut rxy = vec![0.0; n];
    for i in 0..n {
        for j in 0..expected
            .len()
            .min(signal.len().saturating_sub(i))
        {
            if j + i < signal.len() {
                rxy[i] += expected[j] * signal[j + i];
            }
        }
    }

    // Levinson-Durbin algorithm
    let mut coeffs = vec![0.0; n];

    if rxx[0] == 0.0 {
        return coeffs;
    }

    coeffs[0] = rxy[0] / rxx[0];
    let mut e = rxx[0];

    for m in 1..n {
        // Compute reflection coefficient
        let mut k = rxy[m];
        for j in 0..m {
            k -= coeffs[j] * rxx[m - j];
        }
        k /= e;

        // Update coefficients
        let mut new_coeffs = coeffs.clone();
        new_coeffs[m] = k;
        for j in 0..m {
            new_coeffs[j] = coeffs[j] - k * coeffs[m - 1 - j];
        }
        coeffs = new_coeffs;

        // Update error
        e *= 1.0 - k * k;
        if e <= 0.0 {
            break;
        }
    }

    coeffs
}
