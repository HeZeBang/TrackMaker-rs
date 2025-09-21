/// PSK (Phase Shift Keying) modulation and demodulation implementation
use std::f32::consts::PI;

pub struct PskModulator {
    sample_rate: f32,
    carrier_freq: f32,
    symbols_per_second: f32,
    samples_per_symbol: usize,
}

impl PskModulator {
    pub fn new(sample_rate: f32, carrier_freq: f32, symbols_per_second: f32) -> Self {
        let samples_per_symbol = (sample_rate / symbols_per_second) as usize;
        Self {
            sample_rate,
            carrier_freq,
            symbols_per_second,
            samples_per_symbol,
        }
    }

    /// Generate BPSK modulated signal from bits
    pub fn modulate_bpsk(&self, bits: &[u8]) -> Vec<f32> {
        let mut signal = Vec::with_capacity(bits.len() * self.samples_per_symbol);
        
        for (symbol_idx, &bit) in bits.iter().enumerate() {
            // Phase: 0 for bit 0, π for bit 1
            let phase_offset = if bit == 0 { 0.0 } else { PI };
            
            for sample_idx in 0..self.samples_per_symbol {
                let t = (symbol_idx * self.samples_per_symbol + sample_idx) as f32 / self.sample_rate;
                let phase = 2.0 * PI * self.carrier_freq * t + phase_offset;
                signal.push(phase.cos()); // Using cosine for BPSK
            }
        }
        
        signal
    }

    /// Generate QPSK modulated signal from bits (2 bits per symbol)
    pub fn modulate_qpsk(&self, bits: &[u8]) -> Vec<f32> {
        let mut signal = Vec::with_capacity((bits.len() / 2) * self.samples_per_symbol);
        
        for chunk in bits.chunks(2) {
            // Map 2 bits to phase: 00->0°, 01->90°, 10->180°, 11->270°
            let phase_offset = match chunk {
                [0, 0] => 0.0,
                [0, 1] => PI / 2.0,
                [1, 0] => PI,
                [1, 1] => 3.0 * PI / 2.0,
                [bit] => if *bit == 0 { 0.0 } else { PI }, // Handle odd number of bits
                _ => 0.0,
            };
            
            for sample_idx in 0..self.samples_per_symbol {
                let symbol_start = signal.len() / self.samples_per_symbol;
                let t = (symbol_start * self.samples_per_symbol + sample_idx) as f32 / self.sample_rate;
                let phase = 2.0 * PI * self.carrier_freq * t + phase_offset;
                signal.push(phase.cos());
            }
        }
        
        signal
    }
}

pub struct PskDemodulator {
    sample_rate: f32,
    carrier_freq: f32,
    symbols_per_second: f32,
    samples_per_symbol: usize,
    // Reference signals for correlation
    ref_signal_0: Vec<f32>,
    ref_signal_1: Vec<f32>,
}

impl PskDemodulator {
    pub fn new(sample_rate: f32, carrier_freq: f32, symbols_per_second: f32) -> Self {
        let samples_per_symbol = (sample_rate / symbols_per_second) as usize;
        
        // Generate reference signals for correlation-based demodulation
        let mut ref_signal_0 = Vec::with_capacity(samples_per_symbol);
        let mut ref_signal_1 = Vec::with_capacity(samples_per_symbol);
        
        for i in 0..samples_per_symbol {
            let t = i as f32 / sample_rate;
            let phase_0 = 2.0 * PI * carrier_freq * t; // 0° phase
            let phase_1 = 2.0 * PI * carrier_freq * t + PI; // 180° phase
            
            ref_signal_0.push(phase_0.cos());
            ref_signal_1.push(phase_1.cos());
        }
        
        Self {
            sample_rate,
            carrier_freq,
            symbols_per_second,
            samples_per_symbol,
            ref_signal_0,
            ref_signal_1,
        }
    }

    /// Demodulate BPSK signal using correlation detection
    pub fn demodulate_bpsk(&self, signal: &[f32]) -> Vec<u8> {
        let num_symbols = signal.len() / self.samples_per_symbol;
        let mut bits = Vec::with_capacity(num_symbols);
        
        for symbol_idx in 0..num_symbols {
            let start = symbol_idx * self.samples_per_symbol;
            let end = start + self.samples_per_symbol;
            
            if end <= signal.len() {
                let symbol_samples = &signal[start..end];
                
                // Correlate with both reference signals
                let corr_0: f32 = symbol_samples.iter()
                    .zip(self.ref_signal_0.iter())
                    .map(|(s, r)| s * r)
                    .sum();
                    
                let corr_1: f32 = symbol_samples.iter()
                    .zip(self.ref_signal_1.iter())
                    .map(|(s, r)| s * r)
                    .sum();
                
                // Choose the phase with higher correlation
                bits.push(if corr_0 > corr_1 { 0 } else { 1 });
            }
        }
        
        bits
    }

    /// Alternative demodulation using phase detection
    pub fn demodulate_bpsk_phase(&self, signal: &[f32]) -> Vec<u8> {
        let num_symbols = signal.len() / self.samples_per_symbol;
        let mut bits = Vec::with_capacity(num_symbols);
        
        for symbol_idx in 0..num_symbols {
            let start = symbol_idx * self.samples_per_symbol;
            let end = start + self.samples_per_symbol;
            
            if end <= signal.len() {
                let symbol_samples = &signal[start..end];
                
                // Simple phase detection using I/Q components
                let mut i_sum = 0.0f32;
                let mut q_sum = 0.0f32;
                
                for (i, &sample) in symbol_samples.iter().enumerate() {
                    let t = (start + i) as f32 / self.sample_rate;
                    let carrier_i = (2.0 * PI * self.carrier_freq * t).cos();
                    let carrier_q = (2.0 * PI * self.carrier_freq * t).sin();
                    
                    i_sum += sample * carrier_i;
                    q_sum += sample * carrier_q;
                }
                
                // Determine phase based on I component sign
                bits.push(if i_sum >= 0.0 { 0 } else { 1 });
            }
        }
        
        bits
    }
}

/// Utility functions for PSK
pub mod utils {
    use super::*;

    /// Generate chirp preamble for synchronization
    pub fn generate_chirp_preamble(
        sample_rate: f32,
        start_freq: f32,
        end_freq: f32,
        duration_samples: usize,
    ) -> Vec<f32> {
        let mut preamble = Vec::with_capacity(duration_samples);
        let duration_sec = duration_samples as f32 / sample_rate;
        
        for i in 0..duration_samples {
            let t = i as f32 / sample_rate;
            let freq = start_freq + (end_freq - start_freq) * t / duration_sec;
            let phase = 2.0 * PI * start_freq * t + 
                       PI * (end_freq - start_freq) * t * t / duration_sec;
            preamble.push(phase.sin());
        }
        
        preamble
    }

    /// Cross-correlation for synchronization
    pub fn cross_correlate(signal: &[f32], template: &[f32]) -> Vec<f32> {
        let result_len = signal.len().saturating_sub(template.len()) + 1;
        let mut correlation = Vec::with_capacity(result_len);
        
        for i in 0..result_len {
            let corr: f32 = signal[i..i + template.len()]
                .iter()
                .zip(template.iter())
                .map(|(s, t)| s * t)
                .sum();
            correlation.push(corr);
        }
        
        correlation
    }

    /// Find peak in correlation
    pub fn find_correlation_peak(correlation: &[f32]) -> Option<(usize, f32)> {
        correlation
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(idx, &val)| (idx, val))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bpsk_modulation_demodulation() {
        let sample_rate = 48000.0;
        let carrier_freq = 10000.0;
        let symbol_rate = 1000.0;
        
        let modulator = PskModulator::new(sample_rate, carrier_freq, symbol_rate);
        let demodulator = PskDemodulator::new(sample_rate, carrier_freq, symbol_rate);
        
        let test_bits = vec![1, 0, 1, 1, 0, 0, 1, 0];
        let modulated = modulator.modulate_bpsk(&test_bits);
        let demodulated = demodulator.demodulate_bpsk(&modulated);
        
        assert_eq!(test_bits, demodulated);
    }
}
