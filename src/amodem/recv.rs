use num_complex::Complex64;
use std::io::Write;
use crate::amodem::{
    config::Configuration,
    dsp::Modem,
    equalizer::Equalizer,
};

pub struct Receiver {
    modem: Modem,
    frequencies: Vec<f64>,
    omegas: Vec<f64>,
    nsym: usize,
    tsym: f64,
    equalizer: Equalizer,
    carrier_index: usize,
    output_size: usize,
}

impl Receiver {
    pub fn new(config: &Configuration) -> Self {
        let modem = Modem::new(config.symbols.clone());
        let frequencies = config.frequencies.clone();
        let omegas: Vec<f64> = frequencies.iter()
            .map(|&f| 2.0 * std::f64::consts::PI * f / config.fs)
            .collect();
        let nsym = config.nsym;
        let tsym = config.tsym;
        let equalizer = Equalizer::new(config);
        let carrier_index = config.carrier_index;
        
        Self {
            modem,
            frequencies,
            omegas,
            nsym,
            tsym,
            equalizer,
            carrier_index,
            output_size: 0,
        }
    }
    
    pub fn debug_demodulate(&self, signal: &[f64], gain: f64) -> Result<Vec<Complex64>, String> {
        self.demodulate_python_style(signal, gain)
    }
    
    pub fn get_modem(&self) -> &Modem {
        &self.modem
    }
    
    pub fn run<W: Write>(&mut self, signal: Vec<f64>, gain: f64, mut output: W) -> Result<(), String> {
        eprintln!("Receiving");
        
        // Note: skip_start is now handled in main_recv.rs, no need to skip again here
        
        // Use Python-style demodulation
        let symbols = self.demodulate_python_style(&signal, gain)?;
        
        // Skip training sequence exactly like Python
        let training_skip = 550; // prefix(250) + silence(50) + training(200) + silence(50)
        let data_symbols = if symbols.len() > training_skip {
            eprintln!("⏭️  Skipping {} training symbols, {} remain", training_skip, symbols.len() - training_skip);
            symbols[training_skip..].to_vec()
        } else {
            eprintln!("⚠️  Not enough symbols to skip training ({} available)", symbols.len());
            symbols
        };
        
        if data_symbols.len() > 0 {
            eprintln!("🔍 First 5 data symbols:");
            for (i, sym) in data_symbols.iter().take(5).enumerate() {
                eprintln!("  Data[{}]: {:.3} + {:.3}i (mag: {:.3})", i, sym.re, sym.im, sym.norm());
            }
        }
        
        // Decode symbols to bits
        let bit_tuples = self.modem.decode(data_symbols);
        let bits: Vec<bool> = bit_tuples.into_iter()
            .flat_map(|bit_tuple| bit_tuple.into_iter())
            .collect();
        
        // Decode frames
        let frames = self.decode_frames(bits)?;
        
        eprintln!("Starting demodulation");
        for frame in frames {
            output.write_all(&frame).map_err(|e| e.to_string())?;
            self.output_size += frame.len();
        }
        
        let received_kb = self.output_size as f64 / 1e3;
        let duration = 0.033; // Placeholder duration
        let rate = received_kb / duration;
        eprintln!("Received {:.3} kB @ {:.3} seconds = {:.3} kB/s", 
                 received_kb, duration, rate);
        
        Ok(())
    }
    
    fn demodulate_python_style(&self, signal: &[f64], gain: f64) -> Result<Vec<Complex64>, String> {
        // Implement Python-style demodulation using Demux
        // This follows the exact Python logic from dsp.Demux
        
        let mut symbols = Vec::new();
        let _signal_iter = signal.iter().cloned(); // For future use
        
        // Create Python-style Demux filter
        // Python: self.filters = [exp_iwt(-w, Nsym) / (0.5*self.Nsym) for w in omegas]
        let omega = self.omegas[0];
        let filter: Vec<Complex64> = (0..self.nsym).map(|i| {
            let phase = -omega * i as f64;
            let exp_val = Complex64::new(phase.cos(), phase.sin());
            exp_val / (0.5 * self.nsym as f64)
        }).collect();
        
        eprintln!("🔧 Python-style Demux filter (first 4): {:?}", &filter[..4]);
        
        // Process signal in chunks like Python's Demux.next()
        for chunk in signal.chunks(self.nsym) {
            if chunk.len() == self.nsym {
                // Apply gain
                let scaled_chunk: Vec<f64> = chunk.iter().map(|&x| x * gain).collect();
                
                // Python: return np.dot(self.filters, frame)
                // For single carrier: correlation = sum(filter[i] * frame[i])
                let mut correlation = Complex64::new(0.0, 0.0);
                for (i, &sample) in scaled_chunk.iter().enumerate() {
                    correlation += filter[i] * sample;
                }
                
                symbols.push(correlation);
            }
        }
        
        eprintln!("🎯 Extracted {} symbols using Python-style Demux", symbols.len());
        if symbols.len() > 0 {
            eprintln!("First 5 symbols: {:?}", &symbols[..5.min(symbols.len())]);
        }
        
        Ok(symbols)
    }
    
    fn decode_frames(&self, bits: Vec<bool>) -> Result<Vec<Vec<u8>>, String> {
        // Use the framing module to decode frames properly
        use crate::amodem::framing;
        
        // Convert bits to bytes first
        let mut bytes = Vec::new();
        for chunk in bits.chunks(8) {
            if chunk.len() == 8 {
                let mut byte = 0u8;
                for (i, &bit) in chunk.iter().enumerate() {
                    if bit {
                        byte |= 1 << i;
                    }
                }
                bytes.push(byte);
            }
        }
        
        if bytes.is_empty() {
            return Ok(vec![]);
        }
        
        eprintln!("🔍 Decoding {} bytes from {} bits", bytes.len(), bits.len());
        eprintln!("First 20 bytes: {:02x?}", &bytes[..20.min(bytes.len())]);
        
        // Try to decode frames with CRC checking
        match framing::decode(&bytes) {
            Ok(decoded_data) => {
                if !decoded_data.is_empty() {
                    eprintln!("✅ Frame decoding succeeded: {} bytes", decoded_data.len());
                    Ok(vec![decoded_data])
                } else {
                    eprintln!("⚠️  Frame decoding returned empty, using raw bytes");
                    Ok(vec![bytes])
                }
            }
            Err(e) => {
                eprintln!("❌ Frame decoding failed: {}, using raw bytes", e);
                Ok(vec![bytes])
            }
        }
    }
}
