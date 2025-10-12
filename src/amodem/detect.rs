use crate::amodem::{common, config::Configuration, equalizer};
use num_complex::Complex64;
use std::{collections::VecDeque, num::NonZero};

pub struct Detector {
    freq: f64,
    omega: f64,
    nsym: usize,
    tsym: f64,
    maxlen: usize,
    max_offset: usize,

    // Detection thresholds
    coherence_threshold: f64,
    carrier_duration: usize,
    carrier_threshold: usize,
    search_window: usize,
    start_pattern_length: usize,
}

impl Detector {
    pub fn new(config: &Configuration) -> Self {
        let freq = config.fc;
        let omega = 2.0 * std::f64::consts::PI * freq / config.fs;
        let nsym = config.nsym;
        let tsym = config.tsym;
        let maxlen = config.baud; // 1 second of symbols
        let max_offset = (config.timeout * config.fs) as usize;

        // Python constants
        let coherence_threshold = 0.9;
        let carrier_duration = equalizer::get_prefix()
            .iter()
            .sum::<f64>() as usize;
        let carrier_threshold = (0.9 * carrier_duration as f64) as usize;
        let search_window = (0.1 * carrier_duration as f64) as usize;
        let start_pattern_length = search_window / 4;

        Self {
            freq,
            omega,
            nsym,
            tsym,
            maxlen,
            max_offset,
            coherence_threshold,
            carrier_duration,
            carrier_threshold,
            search_window,
            start_pattern_length,
        }
    }

    pub fn run(
        &self,
        mut samples: impl Iterator<Item = f64>,
    ) -> Result<(Vec<f64>, f64, f64), String> {
        // Wait for carrier detection
        let (offset, mut bufs) = self.wait_for_carrier(&mut samples)?;

        // Calculate start time
        let length = (self.carrier_threshold - 1) * self.nsym;
        let begin = offset - length;
        let start_time = (begin as f64) * self.tsym / (self.nsym as f64);
        eprintln!(
            "Carrier detected at ~{:.1} ms @ {:.1} kHz",
            start_time,
            self.freq / 1e3
        );
        eprintln!("Buffered {} ms of audio", bufs.len());

        // Append trailing samples to ensure enough data for precise timing
        let keep = self.carrier_threshold + self.search_window;
        if bufs.len() > keep {
            bufs.drain(0..(bufs.len() - keep));
        }

        let n =
            self.search_window + self.carrier_duration - self.carrier_threshold;
        let mut trailing: Vec<f64> = Vec::with_capacity(n * self.nsym);
        for _ in 0..(n * self.nsym) {
            if let Some(v) = samples.next() {
                trailing.push(v);
            } else {
                break;
            }
        }

        bufs.push(trailing);

        // Flatten buffers and find precise start
        let mut buf: Vec<f64> = bufs
            .into_iter()
            .flatten()
            .collect();
        let offset = self.find_start(&buf);
        let start_time = start_time
            + (offset as f64 / self.nsym as f64 - self.search_window as f64)
                * self.tsym;
        eprintln!("Carrier starts at {:.3} ms", start_time * 1e3);

        buf = buf[offset..].to_vec();

        // Estimate amplitude and frequency error
        let prefix_length = (self.carrier_duration * self.nsym).min(buf.len());
        let (amplitude, freq_err) = self.estimate(&buf[..prefix_length]);

        let mut final_signal = buf;
        final_signal.extend(samples);
        Ok((final_signal, amplitude, freq_err))
    }

    fn wait_for_carrier(
        &self,
        samples: impl Iterator<Item = f64>,
    ) -> Result<(usize, Vec<Vec<f64>>), String> {
        let mut counter = 0;
        let mut bufs = VecDeque::with_capacity(self.maxlen);

        for (offset, buf) in common::iterate(samples, self.nsym).enumerate() {
            if offset * self.nsym > self.max_offset {
                return Err("Timeout waiting for carrier".to_string());
            }

            if bufs.len() >= self.maxlen {
                bufs.pop_front();
            }
            bufs.push_back(buf);

            let coeff = self.coherence(&bufs.back().unwrap());
            if coeff.norm() > self.coherence_threshold {
                counter += 1;
            } else {
                counter = 0;
            }

            if counter == self.carrier_threshold {
                return Ok((offset * self.nsym, bufs.into_iter().collect()));
            }
        }

        Err("No carrier detected".to_string())
    }

    fn coherence(&self, buf: &[f64]) -> Complex64 {
        let n = buf.len();
        if n == 0 {
            return Complex64::new(0.0, 0.0);
        }

        let scale = (0.5 * n as f64).sqrt();
        let mut dot = Complex64::new(0.0, 0.0);
        let mut norm_x_sq = 0.0;

        for (i, &x) in buf.iter().enumerate() {
            let phase = -self.omega * i as f64;
            // e^{i*phase} == cos(phase) + i*sin(phase)
            let h = Complex64::new(phase.cos(), phase.sin()) / scale;
            dot += h * x;

            norm_x_sq += x * x;
        }

        let norm_x = norm_x_sq.sqrt();
        if norm_x == 0.0 {
            Complex64::new(0.0, 0.0)
        } else {
            dot / norm_x
        }
    }

    fn find_start(&self, buf: &[f64]) -> usize {
        let m = self.nsym * self.start_pattern_length;
        if m == 0 {
            return 0;
        }
        let mut carrier: Vec<Complex64> = Vec::with_capacity(m);
        for _ in 0..self.start_pattern_length {
            for i in 0..self.nsym {
                let phase = self.omega * i as f64;
                carrier.push(Complex64::new(phase.cos(), phase.sin()));
            }
        }

        let zeroes_len = m;
        let signal_length: usize = 2 * m;
        let mut signal: Vec<Complex64> = Vec::with_capacity(signal_length);
        signal.resize(zeroes_len, Complex64::new(0.0, 0.0));
        signal.extend_from_slice(&carrier);

        // signal = sqrt(2) * signal / norm(signal)
        let sig_energy = signal
            .iter()
            .map(|z| z.norm_sqr())
            .sum::<f64>()
            .sqrt();
        if sig_energy > 0.0 {
            let scale = (2.0f64).sqrt() / sig_energy;
            for z in signal.iter_mut() {
                *z *= scale;
            }
        }

        if buf.len() < signal_length {
            return 0;
        }

        let out_len = buf.len() - signal_length + 1;

        let mut norm_b: Vec<f64> = vec![0.0; out_len];
        let mut sumsq = 0.0;
        for &x in &buf[..signal_length] {
            sumsq += x * x;
        }
        norm_b[0] = sumsq.sqrt();
        for i in 1..out_len {
            sumsq += buf[i + signal_length - 1] * buf[i + signal_length - 1]
                - buf[i - 1] * buf[i - 1];
            norm_b[i] = if sumsq > 0.0 { sumsq.sqrt() } else { 0.0 };
        }

        // corr = abs(correlate(buf, signal))
        // coeffs = corr / norm_b (where norm_b > 0)
        let mut best_idx = 0usize;
        let mut best_coeff = f64::MIN;
        for i in 0..out_len {
            if norm_b[i] == 0.0 {
                continue;
            }
            let mut acc = Complex64::new(0.0, 0.0);
            // dot = sum_k buf[i+k] * signal[k]
            for k in 0..signal_length {
                acc += signal[k] * buf[i + k];
            }
            let coeff = acc.norm() / norm_b[i];
            if coeff > best_coeff {
                best_coeff = coeff;
                best_idx = i;
            }
        }

        eprintln!("Carrier coherence: {:.3}%", best_coeff * 100.0);
        best_idx + zeroes_len
    }

    fn estimate(&self, buf: &[f64]) -> (f64, f64) {
        let scale = 0.5 * self.nsym as f64;
        let mut filt: Vec<Complex64> = Vec::with_capacity(self.nsym);
        for i in 0..self.nsym {
            let phase = -self.omega * i as f64;
            filt.push(Complex64::new(phase.cos(), phase.sin()) / scale);
        }

        let mut symbols: Vec<Complex64> = Vec::new();
        for frame in buf.chunks(self.nsym) {
            if frame.len() < self.nsym {
                break;
            }
            let mut sum = Complex64::new(0.0, 0.0);
            for (i, &x) in frame.iter().enumerate() {
                sum += filt[i] * x;
            }
            symbols.push(sum);
        }

        let skip = 5;
        let symbols = if symbols.len() > 2 * skip {
            &symbols[skip..symbols.len() - skip]
        } else {
            &symbols[..]
        };
        if symbols.is_empty() {
            return (1.0, 0.0);
        }

        let amplitude = symbols
            .iter()
            .map(|c| c.norm())
            .sum::<f64>()
            / symbols.len() as f64;
        eprintln!("Carrier symbols amplitude: {:.3}", amplitude);

        let mut phases: Vec<f64> = symbols
            .iter()
            .map(|c| c.arg())
            .collect();

        for i in 1..phases.len() {
            while phases[i] - phases[i - 1] > std::f64::consts::PI {
                phases[i] -= 2.0 * std::f64::consts::PI;
            }
            while phases[i] - phases[i - 1] < -std::f64::consts::PI {
                phases[i] += 2.0 * std::f64::consts::PI;
            }
        }
        for phase in phases.iter_mut() {
            *phase /= 2.0 * std::f64::consts::PI;
        }

        let n = phases.len() as f64;
        let x_mean = (n - 1.0) / 2.0;
        let y_mean = phases.iter().sum::<f64>() / n;
        let mut num = 0.0;
        let mut den = 0.0;
        for (i, &y) in phases.iter().enumerate() {
            let xi = i as f64;
            num += (xi - x_mean) * (y - y_mean);
            den += (xi - x_mean).powi(2);
        }
        let a = if den != 0.0 { num / den } else { 0.0 };
        let freq_err = a / (self.tsym * self.freq);
        eprintln!("Frequency error: {:.3} ppm", freq_err * 1e6);
        (amplitude, freq_err)
    }
}
