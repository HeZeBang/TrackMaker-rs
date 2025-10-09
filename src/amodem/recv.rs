use num_complex::Complex64;
use std::io::Write;
use crate::amodem::{
    config::Configuration,
    dsp::{Demux, Modem},
    equalizer::Equalizer,
    sampling::Sampler,
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
        self.demodulate_python_style(signal, gain, 1.0)
    }
    
    pub fn get_modem(&self) -> &Modem {
        &self.modem
    }
    
    pub fn run<W: Write>(&mut self, signal: Vec<f64>, gain: f64, freq: f64, mut output: W) -> Result<(), String> {
        eprintln!("Receiving");

        // 迭代式解调：构造采样器与 demux，一边产出符号一边处理
        let sampler = Sampler::new(&signal, freq);
        let mut demux = Demux::new(sampler, &self.omegas, self.nsym, gain);

        // 先收集足够的前导 + 训练段用于跳过（保持与 Python 对齐）
        let training_skip = 550usize;
        let mut data_symbols: Vec<Complex64> = Vec::new();
        let mut consumed = 0usize;
        while let Some(row) = demux.next() {
            let carrier_symbol = row
                .get(self.carrier_index)
                .copied()
                .or_else(|| row.first().copied())
                .unwrap_or(Complex64::new(0.0, 0.0));
            consumed += 1;
            if consumed > training_skip {
                data_symbols.push(carrier_symbol);
            }
        }

        if !data_symbols.is_empty() {
            eprintln!("🔍 First 5 data symbols:");
            for (i, sym) in data_symbols.iter().take(5).enumerate() {
                eprintln!("  Data[{}]: {:.3} + {:.3}i (mag: {:.3})", i, sym.re, sym.im, sym.norm());
            }
        }

        // 将符号逐步映射成比特（拉平成单一位流）
        let bit_tuples = self.modem.decode(data_symbols);
        let bits_iter = bit_tuples.into_iter().flat_map(|tuple| tuple.into_iter());

        // 使用基于位流的帧解码器，遇到 EOF 自动结束
        let mut frames_iter = crate::amodem::framing::decode_frames_from_bits(bits_iter);
        eprintln!("Starting demodulation");
        while let Some(frame) = frames_iter.next() {
            output.write_all(&frame).map_err(|e| e.to_string())?;
            self.output_size += frame.len();
        }

        // 简要统计
        let received_kb = self.output_size as f64 / 1e3;
        eprintln!("Received {:.3} kB", received_kb);
        Ok(())
    }
    
    fn demodulate_python_style(&self, signal: &[f64], gain: f64, freq: f64) -> Result<Vec<Complex64>, String> {
        if self.omegas.is_empty() {
            return Err("Receiver has no configured carriers".to_string());
        }

        let sampler = Sampler::new(signal, freq);
        let mut demux = Demux::new(sampler, &self.omegas, self.nsym, gain);
        let mut symbols = Vec::new();

        while let Some(symbol_row) = demux.next() {
            let carrier_symbol = symbol_row
                .get(self.carrier_index)
                .copied()
                .or_else(|| symbol_row.first().copied())
                .unwrap_or_else(|| Complex64::new(0.0, 0.0));
            symbols.push(carrier_symbol);
        }

        eprintln!("🎯 Extracted {} symbols using Python-style Demux", symbols.len());
        if symbols.len() > 0 {
            eprintln!("First 5 symbols: {:?}", &symbols[..5.min(symbols.len())]);
        }

        Ok(symbols)
    }
    
    fn decode_frames(&self, _bits: Vec<bool>) -> Result<Vec<Vec<u8>>, String> {
        // 不再使用，保留签名以最小侵入
        Ok(vec![])
    }
}
