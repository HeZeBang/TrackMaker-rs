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

    pub fn train_symbols(&self, length: usize) -> Vec<Vec<Complex64>> {
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
    let padding = vec![0.0; lookahead];
    assert!(
        signal.len() == expected.len(),
        "Signal length {} must be equal to expected length {}",
        signal.len(),
        expected.len()
    );
    let x = [&signal[..], &padding[..]].concat();
    let y = [&padding[..], &expected[..]].concat();

    let n = order + lookahead; // filter length
    let mut rxx = vec![0.0; n];
    let mut rxy = vec![0.0; n];

    for i in 0..n {
        for k in 0..(x.len() - i) {
            rxx[i] += x[i + k] * x[k];
            rxy[i] += y[i + k] * x[k];
        }
    }

    levinson_solve(&rxx, &rxy)
}

fn levinson_solve(t: &[f64], y: &[f64]) -> Vec<f64> {
    let n_len = t.len();
    assert_eq!(y.len(), n_len, "Input vectors must have same length");
    assert!(n_len > 0 && (t[0].abs() > 1e-10), "t[0] must be non-zero");

    // Initialize forward and backward vectors
    let t0_inv = 1.0 / t[0];
    let mut f_vecs: Vec<Vec<f64>> = vec![vec![t0_inv]];
    let mut b_vecs: Vec<Vec<f64>> = vec![vec![t0_inv]];

    // Build forward and backward prediction-error filter vectors
    for n in 1..n_len {
        let prev_f = &f_vecs[n - 1];
        let prev_b = &b_vecs[n - 1];

        // Calculate reflection coefficients
        let ef: f64 = (0..n)
            .map(|i| t[n - i] * prev_f[i])
            .sum();
        let eb: f64 = (0..n)
            .map(|i| t[i + 1] * prev_b[i])
            .sum();

        let det = 1.0 - ef * eb;
        assert!(
            det.abs() > 1e-10,
            "Determinant too small, matrix may be singular"
        );

        // Update forward vector
        let mut f_new = prev_f.clone();
        f_new.push(0.0);
        let mut b_new = vec![0.0];
        b_new.extend(prev_b.clone());

        let f_next: Vec<f64> = (0..=n)
            .map(|i| (f_new[i] - ef * b_new[i]) / det)
            .collect();

        let b_next: Vec<f64> = (0..=n)
            .map(|i| (b_new[i] - eb * f_new[i]) / det)
            .collect();

        f_vecs.push(f_next);
        b_vecs.push(b_next);
    }

    // Solve for coefficients x using forward substitution
    let mut x = Vec::new();
    for n in 0..n_len {
        x.push(0.0);
        let ef: f64 = (0..n)
            .map(|i| t[n - i] * x[i])
            .sum();
        let scale = y[n] - ef;
        let b_n = &b_vecs[n];
        for (i, &b_val) in b_n.iter().enumerate() {
            if i < x.len() {
                x[i] += scale * b_val;
            } else {
                x.push(scale * b_val);
            }
        }
    }

    x
}
