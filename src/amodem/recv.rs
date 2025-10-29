use crate::amodem::{
    config::Configuration,
    dsp::{Demux, Modem},
    equalizer::{Equalizer, get_prefix},
    sampling::Sampler,
};
use num_complex::Complex64;
use std::io::Write;
use tracing::{debug, info};

pub struct Receiver {
    modem: Modem,
    frequencies: Vec<f64>,
    omegas: Vec<f64>,
    nsym: usize,
    tsym: f64,
    equalizer: Equalizer,
    carrier_index: usize,
    /// number of bytes written to output stream
    output_size: usize,
    use_reed_solomon: bool,
    iters_per_update: usize, // ms
    iters_per_report: usize, // ms
    modem_bitrate: usize,
    /// integration feedback gain
    freq_err_gain: f64,
    ecc_len: usize,
}

impl Receiver {
    pub fn new(config: &Configuration) -> Self {
        Self::with_reed_solomon(config, false, 8)
    }

    pub fn with_reed_solomon(
        config: &Configuration,
        use_reed_solomon: bool,
        ecc_len: usize,
    ) -> Self {
        let modem = Modem::new(config.symbols.clone());
        let frequencies = config.frequencies.clone();
        let omegas: Vec<f64> = frequencies
            .iter()
            .map(|&f| 2.0 * std::f64::consts::PI * f / config.fs)
            .collect();
        let nsym = config.nsym;
        let tsym = config.tsym;
        let iters_per_update = 100;
        let iters_per_report = 1000;
        let modem_bitrate = config.modem_bps;
        let equalizer = Equalizer::new(config);
        let carrier_index = config.carrier_index;
        let freq_err_gain = 0.01 * tsym;

        Self {
            modem,
            frequencies,
            omegas,
            nsym,
            tsym,
            equalizer,
            carrier_index,
            output_size: 0,
            freq_err_gain,
            iters_per_update,
            iters_per_report,
            modem_bitrate,
            use_reed_solomon,
            ecc_len,
        }
    }

    pub fn _prefix(&self, symbols: Vec<Complex64>, gain: Option<f64>) {
        let gain = gain.unwrap_or(1.0);
    }

    pub fn get_modem(&self) -> &Modem {
        &self.modem
    }

    pub fn run<W: Write>(
        &mut self,
        sampler: Sampler,
        gain: f64,
        mut output: W,
    ) -> Result<(), String> {
        debug!("Receiving");
        let mut symbols = Demux::new(sampler, &self.omegas, self.nsym, gain);
        // self._prefix(symbols, gain);

        // 先收集足够的前导 + 训练段用于跳过（保持与 Python 对齐）
        let training_skip = 550usize;
        let mut data_symbols: Vec<Complex64> = Vec::new();
        let mut consumed = 0usize;
        while let Some(row) = symbols.next() {
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
            for (i, sym) in data_symbols
                .iter()
                .take(5)
                .enumerate()
            {
                eprintln!(
                    "  Data[{}]: {:.3} + {:.3}i (mag: {:.3})",
                    i,
                    sym.re,
                    sym.im,
                    sym.norm()
                );
            }
        }

        // 将符号逐步映射成比特（拉平成单一位流）
        let bit_tuples = self
            .modem
            .decode(data_symbols);
        let bits_iter = bit_tuples
            .into_iter()
            .flat_map(|tuple| tuple.into_iter());

        // 使用基于位流的帧解码器，遇到 EOF 自动结束
        eprintln!("Starting demodulation");
        if self.use_reed_solomon {
            let frames_iter = crate::amodem::framing::decode_frames_from_bits_with_reed_solomon(bits_iter, self.ecc_len);
            for frame in frames_iter {
                output
                    .write_all(&frame)
                    .map_err(|e| e.to_string())?;
                self.output_size += frame.len();
            }
        } else {
            let frames_iter =
                crate::amodem::framing::decode_frames_from_bits(bits_iter);
            for frame in frames_iter {
                output
                    .write_all(&frame)
                    .map_err(|e| e.to_string())?;
                self.output_size += frame.len();
            }
        }

        // 简要统计
        let received_kb = self.output_size as f64 / 1e3;
        eprintln!("Received {:.3} kB", received_kb);
        Ok(())
    }
}
