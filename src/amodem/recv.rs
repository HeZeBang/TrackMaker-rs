use crate::amodem::{
    config::Configuration,
    dsp::{Demux, Modem},
    equalizer::Equalizer,
    sampling::Sampler,
};
use num_complex::Complex64;
use std::io::Write;

pub struct Receiver {
    modem: Modem,
    frequencies: Vec<f64>,
    omegas: Vec<f64>,
    nsym: usize,
    tsym: f64,
    equalizer: Equalizer,
    carrier_index: usize,
    output_size: usize,
    use_reed_solomon: bool,
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
            use_reed_solomon,
            ecc_len,
        }
    }

    fn prefix(&self, demux: &mut Demux, gain: f64) -> Result<(), String> {
        let training_skip = 550usize;
        let mut skipped = 0usize;

        while skipped < training_skip {
            if demux.next().is_none() {
                break;
            }
            skipped += 1;
        }

        eprintln!(
            "Prefix phase: skipped {} symbols (gain {:.3})",
            skipped, gain
        );
        Ok(())
    }

    fn collect_symbols_per_carrier(
        &self,
        demux: &mut Demux,
    ) -> Vec<Vec<Complex64>> {
        let n = self.omegas.len();
        let mut per_carrier: Vec<Vec<Complex64>> = vec![Vec::new(); n];

        while let Some(row) = demux.next() {
            for (i, &sym) in row.iter().enumerate() {
                if i < n {
                    per_carrier[i].push(sym);
                }
            }
        }

        per_carrier
    }

    fn decode_interleaved_bits(
        &self,
        per_carrier_syms: Vec<Vec<Complex64>>,
    ) -> Vec<bool> {
        let mut streams: Vec<Vec<Vec<bool>>> =
            Vec::with_capacity(per_carrier_syms.len());
        for syms in per_carrier_syms {
            streams.push(self.modem.decode(syms));
        }

        // Get the minimum length among all streams for zipping
        let min_len = streams
            .iter()
            .map(|s| s.len())
            .min()
            .unwrap_or(0);

        let mut flat_bits: Vec<bool> = Vec::new();
        // zip streams by time, then by carrier, flattening bits
        for t in 0..min_len {
            for c in 0..streams.len() {
                if let Some(bits) = streams[c].get(t) {
                    flat_bits.extend(bits.iter().copied());
                }
            }
        }

        flat_bits
    }

    pub fn run<W: Write>(
        &mut self,
        signal: Vec<f64>,
        gain: f64,
        freq: f64,
        mut output: W,
    ) -> Result<(), String> {
        eprintln!("Receiving");

        let sampler = Sampler::new(&signal, freq);
        let mut demux = Demux::new(sampler, &self.omegas, self.nsym, gain);

        self.prefix(&mut demux, gain)?;

        // NOT IMPLEMENTED TRAIN

        // _bitstream_START
        let per_carrier_syms = self.collect_symbols_per_carrier(&mut demux);

        if !per_carrier_syms.is_empty()
            && self.carrier_index < per_carrier_syms.len()
        {
            let sample_syms = &per_carrier_syms[self.carrier_index];
            eprintln!("🔍 First 5 symbols:");
            for (i, s) in sample_syms
                .iter()
                .take(5)
                .enumerate()
            {
                eprintln!(
                    "  S[{}]: {:.3} + {:.3}i (mag: {:.3})",
                    i,
                    s.re,
                    s.im,
                    s.norm()
                );
            }
        }

        let flat_bits = self.decode_interleaved_bits(per_carrier_syms);
        let bits_iter = flat_bits.into_iter();
        // _bitstream_END

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

        let received_kb = self.output_size as f64 / 1e3;
        eprintln!("Received {:.3} kB", received_kb);
        Ok(())
    }
}
