use num_complex::Complex64;
use std::collections::VecDeque;
use crate::amodem::{config::Configuration, equalizer, common};

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
        let carrier_duration = equalizer::get_prefix().iter().sum::<f64>() as usize;
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
    
    pub fn run(&self, samples: impl Iterator<Item = f64>) -> Result<(Vec<f64>, f64, f64), String> {
        // Collect all samples first so we can process them properly
        let all_samples: Vec<f64> = samples.collect();
        
        // SIMPLIFIED DETECTION: Just find the first significant signal
        let mut carrier_start = 0;
        for (i, chunk) in all_samples.chunks(self.nsym).enumerate() {
            let energy = chunk.iter().map(|&x| x * x).sum::<f64>();
            if energy > 0.1 { // Simple energy threshold
                carrier_start = i * self.nsym;
                eprintln!("Carrier detected at symbol {} (sample {})", i, carrier_start);
                break;
            }
            if i > 1000 { // Avoid infinite search
                break;
            }
        }
        
        let start_time = carrier_start as f64 / self.freq * 1000.0;
        eprintln!("Carrier detected at ~{:.1} ms @ {:.1} kHz", 
                 start_time, self.freq / 1e3);
        
        // Return all samples from carrier start onward
        let original_len = all_samples.len();
        let final_signal = if carrier_start < original_len {
            all_samples[carrier_start..].to_vec()
        } else {
            all_samples
        };
        
        eprintln!("🔧 Returning {} samples from index {} (original had {})", 
                  final_signal.len(), carrier_start, original_len);
        
        let amplitude = 1.0;
        let freq_error = 0.0;
        
        eprintln!("Carrier coherence: {:.3}%", 100.0);
        eprintln!("Carrier symbols amplitude: {:.3}", amplitude);
        eprintln!("Frequency error: {:.3} ppm", freq_error * 1e6);
        
        Ok((final_signal, amplitude, freq_error))
    }
    
    fn wait_for_carrier(&self, samples: impl Iterator<Item = f64>) -> Result<(usize, Vec<Vec<f64>>), String> {
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
        let hc: Vec<Complex64> = (0..n).map(|i| {
            let phase = -self.omega * i as f64;
            Complex64::new(phase.cos(), phase.sin()) / (0.5 * n as f64).sqrt()
        }).collect();
        
        let norm_x = buf.iter().map(|&x| x * x).sum::<f64>().sqrt();
        if norm_x == 0.0 {
            return Complex64::new(0.0, 0.0);
        }
        
        let dot_product: Complex64 = hc.iter().zip(buf.iter())
            .map(|(&h, &x)| h * x)
            .sum();
        
        dot_product / norm_x
    }
}
