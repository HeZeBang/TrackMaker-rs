use std::collections::HashMap;

use num_complex::Complex64;

use crate::amodem::common;

pub struct Fir {
    h: Vec<f64>,
    state: Vec<f64>,
}

impl Fir {
    pub fn new(h: Vec<f64>) -> Self {
        let len = h.len();
        Self {
            h,
            state: vec![0.0; len],
        }
    }

    pub fn process(&mut self, input: Vec<f64>) -> Vec<f64> {
        let mut output = Vec::new();
        for v in input {
            for idx in (1..self.state.len()).rev() {
                self.state[idx] = self.state[idx - 1];
            }
            if !self.state.is_empty() {
                self.state[0] = v;
            }
            let value = self
                .state
                .iter()
                .zip(self.h.iter())
                .map(|(x, h)| x * h)
                .sum();
            output.push(value);
        }
        output
    }
}

pub struct Demux<S: FnMut(usize) -> Option<Vec<f64>>> {
    sample_fn: S,
    filters: Vec<Vec<Complex64>>,
    nsym: usize,
    gain: f64,
}

impl<S: FnMut(usize) -> Option<Vec<f64>>> Demux<S> {
    pub fn new(sample_fn: S, omegas: &[f64], nsym: usize, gain: f64) -> Self {
        let norm = 0.5 * nsym as f64;
        let filters = omegas
            .iter()
            .map(|&omega| {
                exp_iwt(-omega, nsym)
                    .into_iter()
                    .map(|c| c / norm)
                    .collect()
            })
            .collect();

        Self {
            sample_fn,
            filters,
            nsym,
            gain,
        }
    }

    pub fn filters(&self) -> &[Vec<Complex64>] {
        &self.filters
    }
}

impl<S: FnMut(usize) -> Option<Vec<f64>>> Iterator for Demux<S> {
    type Item = Vec<Complex64>;

    fn next(&mut self) -> Option<Self::Item> {
        let mut frame = (self.sample_fn)(self.nsym)?;
        if (self.gain - 1.0).abs() > f64::EPSILON {
            for value in &mut frame {
                *value *= self.gain;
            }
        }

        let mut result = Vec::with_capacity(self.filters.len());
        for filter in &self.filters {
            let mut acc = Complex64::new(0.0, 0.0);
            for (coeff, sample) in filter
                .iter()
                .zip(frame.iter())
            {
                acc += *coeff * *sample;
            }
            result.push(acc);
        }
        Some(result)
    }
}

pub fn exp_iwt(omega: f64, n: usize) -> Vec<Complex64> {
    (0..n)
        .map(|i| {
            let phase = omega * i as f64;
            Complex64::new(phase.cos(), phase.sin())
        })
        .collect()
}

pub fn norm(x: &[Complex64]) -> f64 {
    x.iter()
        .map(|v| v.norm_sqr())
        .sum::<f64>()
        .sqrt()
}

pub fn rms(x: &[Complex64]) -> f64 {
    if x.is_empty() {
        return 0.0;
    }
    let mean = x
        .iter()
        .map(|v| v.norm_sqr())
        .sum::<f64>()
        / x.len() as f64;
    mean.sqrt()
}

/// Calculate RMS along the first axis (time axis) for 2D array
/// Returns RMS for each column (frequency)
pub fn rms_2d(x: &[Vec<Complex64>]) -> Vec<f64> {
    if x.is_empty() || x[0].is_empty() {
        return Vec::new();
    }

    let n_cols = x[0].len();
    let mut result = vec![0.0; n_cols];

    // For each column (frequency)
    for col in 0..n_cols {
        let mut sum_sq = 0.0;
        let mut count = 0;

        // Sum squared magnitudes along the column
        for row in x {
            if col < row.len() {
                sum_sq += row[col].norm_sqr();
                count += 1;
            }
        }

        if count > 0 {
            result[col] = (sum_sq / count as f64).sqrt();
        }
    }

    result
}

pub fn coherence(x: &[f64], omega: f64) -> Complex64 {
    let n = x.len();
    if n == 0 {
        return Complex64::new(0.0, 0.0);
    }
    let hc = exp_iwt(-omega, n);
    let norm_x = x
        .iter()
        .map(|v| v * v)
        .sum::<f64>()
        .sqrt();
    if norm_x == 0.0 {
        return Complex64::new(0.0, 0.0);
    }
    hc.into_iter()
        .zip(x.iter())
        .map(|(c, &sample)| c * sample)
        .sum::<Complex64>()
        / norm_x
}

pub fn linear_regression(x: &[f64], y: &[f64]) -> Option<(f64, f64)> {
    if x.len() != y.len() || x.is_empty() {
        return None;
    }
    let mean_x = x.iter().sum::<f64>() / x.len() as f64;
    let mean_y = y.iter().sum::<f64>() / y.len() as f64;
    let mut num = 0.0;
    let mut den = 0.0;
    for (&xv, &yv) in x.iter().zip(y.iter()) {
        let x_ = xv - mean_x;
        let y_ = yv - mean_y;
        num += y_ * x_;
        den += x_ * x_;
    }
    if den == 0.0 {
        return None;
    }
    let a = num / den;
    let b = mean_y - a * mean_x;
    Some((a, b))
}

pub struct Modem {
    encode_map: HashMap<Vec<bool>, Complex64>,
    decode_list: Vec<(Complex64, Vec<bool>)>,

    symbols: Vec<Complex64>,
    bits_per_symbol: usize,
}

impl Modem {
    pub fn new(symbols: Vec<Complex64>) -> Self {
        let bits_per_symbol = (symbols.len() as f64).log2() as usize;
        assert_eq!(2_usize.pow(bits_per_symbol as u32), symbols.len());

        let mut encode_map = HashMap::new();
        let mut decode_list = Vec::with_capacity(symbols.len());

        for (i, &symbol) in symbols.iter().enumerate() {
            let mut bits = Vec::with_capacity(bits_per_symbol);
            for j in 0..bits_per_symbol {
                bits.push((i & (1 << j)) != 0);
            }
            encode_map.insert(bits.clone(), symbol);
            decode_list.push((symbol, bits));
        }

        Self {
            encode_map,
            decode_list,
            symbols,
            bits_per_symbol,
        }
    }

    pub fn encode(&self, bits: impl Iterator<Item = bool>) -> Vec<Complex64> {
        let bit_vec: Vec<bool> = bits.collect();
        common::iterate(bit_vec.into_iter(), self.bits_per_symbol, None)
            .map(|bit_chunk| {
                self.encode_map
                    .get(&bit_chunk)
                    .copied()
                    .unwrap_or(Complex64::new(0.0, 0.0))
            })
            .collect()
    }

    pub fn bits_per_symbol(&self) -> usize {
        self.bits_per_symbol
    }

    pub fn symbols(&self) -> &[Complex64] {
        &self.symbols
    }

    /// Maximum-likelihood decoding using nearest-neighbor
    /// Returns decoded bits for each symbol
    pub fn decode(&self, symbols: Vec<Complex64>) -> Vec<Vec<bool>> {
        symbols
            .into_iter()
            .map(|received| {
                let mut min_error = f64::INFINITY;
                let mut best_bits = Vec::new();

                for (symbol, bits) in &self.decode_list {
                    let error = (received - *symbol).norm();
                    if error < min_error {
                        min_error = error;
                        best_bits = bits.clone();
                    }
                }
                best_bits
            })
            .collect()
    }

    /// Maximum-likelihood decoding with error handler
    /// Calls error_handler for each symbol with (received, decoded)
    pub fn decode_with_error_handler<F>(
        &self,
        symbols: Vec<Complex64>,
        mut error_handler: F,
    ) -> Vec<Vec<bool>>
    where
        F: FnMut(Complex64, Complex64),
    {
        symbols
            .into_iter()
            .map(|received| {
                let mut min_error = f64::INFINITY;
                let mut best_bits = Vec::new();
                let mut decoded_symbol = Complex64::new(0.0, 0.0);

                for (symbol, bits) in &self.decode_list {
                    let error = (received - *symbol).norm();
                    if error < min_error {
                        min_error = error;
                        best_bits = bits.clone();
                        decoded_symbol = *symbol;
                    }
                }

                // Call error handler with received and decoded symbols
                error_handler(received, decoded_symbol);
                best_bits
            })
            .collect()
    }
}

// Pseudo-random bit sequence generator
pub struct Prbs {
    reg: u32,
    poly: u32,
    mask: u32,
    size: usize,
}

impl Prbs {
    pub fn new(reg: u32, poly: u32, bits: usize) -> Self {
        let mask = (1 << bits) - 1;

        let mut size = 0;
        while (poly >> size) > 1 {
            size += 1;
        }

        Self {
            reg,
            poly,
            mask,
            size,
        }
    }
}

impl Iterator for Prbs {
    type Item = u32;

    fn next(&mut self) -> Option<Self::Item> {
        let result = self.reg & self.mask;
        self.reg <<= 1;
        if self.reg >> self.size != 0 {
            self.reg ^= self.poly;
        }
        Some(result)
    }
}

pub fn prbs(reg: u32, poly: u32, bits: usize) -> Prbs {
    Prbs::new(reg, poly, bits)
}
