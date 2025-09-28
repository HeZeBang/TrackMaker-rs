use num_complex::Complex64;
use std::f64::consts::PI;

#[derive(Clone, Debug)]
pub struct Configuration {
    pub fs: f64,                    // sampling frequency [Hz]  
    pub tsym: f64,                  // symbol duration [seconds]
    pub npoints: usize,             // constellation points
    pub frequencies: Vec<f64>,      // carrier frequencies
    
    // audio config
    pub bits_per_sample: usize,
    pub latency: f64,
    
    // sender config
    pub silence_start: f64,
    pub silence_stop: f64,
    
    // receiver config  
    pub skip_start: f64,
    pub timeout: f64,
    
    // computed values
    pub ts: f64,                    // 1/Fs
    pub fsym: f64,                  // 1/Tsym
    pub nsym: usize,                // int(Tsym / Ts)
    pub baud: usize,                // int(1.0 / Tsym)
    pub nfreq: usize,               // len(frequencies)
    pub carrier_index: usize,
    pub fc: f64,                    // frequencies[carrier_index]
    pub bits_per_baud: usize,
    pub modem_bps: f64,
    pub carriers: Vec<Vec<Complex64>>,
    pub symbols: Vec<Complex64>,
}

impl Configuration {
    pub fn new(fs: f64, npoints: usize, frequencies: Vec<f64>) -> Self {
        let tsym = 0.001; // symbol duration [seconds]
        let bits_per_sample = 16;
        let latency = 0.1;
        let silence_start = 0.5;
        let silence_stop = 0.5;
        let skip_start = 0.1;
        let timeout = 60.0;
        
        // computed values
        let ts = 1.0 / fs;
        let fsym = 1.0 / tsym;
        let nsym = (tsym / ts) as usize;
        let baud = (1.0 / tsym) as usize;
        assert_eq!(baud as f64 * tsym, 1.0);
        
        let nfreq = frequencies.len();
        let carrier_index = 0;
        let fc = frequencies[carrier_index];
        
        let bits_per_symbol = (npoints as f64).log2() as usize;
        assert_eq!(2_usize.pow(bits_per_symbol as u32), npoints);
        let bits_per_baud = bits_per_symbol * nfreq;
        let modem_bps = baud as f64 * bits_per_baud as f64;
        
        // Generate carriers
        let carriers: Vec<Vec<Complex64>> = frequencies.iter().map(|&f| {
            (0..nsym).map(|n| {
                let phase = 2.0 * PI * f * (n as f64) * ts;
                Complex64::new(phase.cos(), phase.sin())
            }).collect()
        }).collect();
        
        // Generate QAM constellation - matching Python exactly
        let nx = 2_usize.pow(((bits_per_symbol / 2) as f64).ceil() as u32);  // 整数除法，然后 ceil
        let ny = npoints / nx;
        let mut symbols = Vec::new();
        for x in 0..nx {
            for y in 0..ny {
                symbols.push(Complex64::new(x as f64, y as f64));
            }
        }
        
        // Center and normalize symbols - matching Python logic
        let last_symbol = symbols[symbols.len() - 1];
        let offset = last_symbol / 2.0;
        for symbol in &mut symbols {
            *symbol -= offset;
        }
        
        let max_abs = symbols.iter()
            .map(|s| s.norm())
            .fold(0.0, f64::max);
        
        for symbol in &mut symbols {
            *symbol /= max_abs;
        }
        
        Self {
            fs, tsym, npoints, frequencies,
            bits_per_sample, latency,
            silence_start, silence_stop,
            skip_start, timeout,
            ts, fsym, nsym, baud, nfreq, carrier_index, fc,
            bits_per_baud, modem_bps,
            carriers, symbols,
        }
    }
    
    pub fn bitrate_1() -> Self {
        Self::new(8000.0, 2, vec![2000.0])
    }
    
    pub fn bitrate_2() -> Self {
        Self::new(8000.0, 4, vec![2000.0])
    }
}
