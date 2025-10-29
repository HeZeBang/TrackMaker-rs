use crate::amodem::{
    config::Configuration,
    dsp::{Demux, Fir, Modem},
    equalizer::{
        EQUALIZER_LENGTH, Equalizer, SILENCE_LENGTH, get_prefix, train,
    },
    sampling::Sampler,
};
use num_complex::Complex64;
use std::io::Write;
use symphonia::core::sample;
use tracing::{debug, info, warn};

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

    fn prefix<S: FnMut(usize) -> Option<Vec<f64>>>(
        &self,
        symbols: &mut Demux<S>,
        gain: f64,
    ) -> Result<(), String> {
        // Get the expected prefix: 200 ones + 50 zeros
        let expected_prefix = get_prefix();
        let prefix_len = expected_prefix.len();

        // Collect symbols for prefix check
        let mut prefix_symbols = Vec::new();
        for _ in 0..prefix_len {
            if let Some(row) = symbols.next() {
                if let Some(&carrier_sym) = row.get(self.carrier_index) {
                    prefix_symbols.push(carrier_sym);
                } else if let Some(&first_sym) = row.first() {
                    prefix_symbols.push(first_sym);
                }
            } else {
                return Err(
                    "Not enough symbols for prefix verification".to_string()
                );
            }
        }

        // Extract magnitude and round
        let received_magnitude: Vec<f64> = prefix_symbols
            .iter()
            .map(|s| (s.norm() * gain).round())
            .collect();

        // Compare with expected prefix
        let mut error_count = 0;
        for (i, (&received, &expected)) in received_magnitude
            .iter()
            .zip(expected_prefix.iter())
            .enumerate()
        {
            if (received as u32) != (expected as u32) {
                error_count += 1;
                warn!(
                    "Prefix error at index {}: got {}, expected {}",
                    i, received, expected
                );
            }
        }

        debug!(
            "Prefix verification: {} errors out of {}",
            error_count, prefix_len
        );

        if error_count > 3 {
            return Err(format!("Incorrect prefix: {} errors", error_count));
        }

        info!("Prefix OK");
        Ok(())
    }

    fn train<I: Iterator<Item = f64>>(
        &self,
        sampler: &mut Sampler<I>,
        order: usize,
        lookahead: usize,
    ) -> Result<Fir, String> {
        // Generate training symbols
        let train_symbols = self
            .equalizer
            .train_symbols(EQUALIZER_LENGTH);

        // Modulate training symbols
        let train_signal = self
            .equalizer
            .modulator(&train_symbols);

        // Extract the relevant portions (skip silence_length*Nsym prefix, remove postfix)
        let prefix = SILENCE_LENGTH * self.nsym;
        let postfix = SILENCE_LENGTH * self.nsym;
        let signal_length = EQUALIZER_LENGTH * self.nsym + prefix + postfix;

        let signal = sampler
            .take(signal_length)
            .ok_or_else(|| {
                "Not enough samples from sampler for equalizer training"
                    .to_string()
            })?;

        let mut expected = train_signal.clone();
        expected.extend(vec![0.0; lookahead]);

        // Compute filter coefficients using Levinson-Durbin
        let coeffs = train(&signal, &expected, order, lookahead);

        // Plot coefficients using plotly
        self.plot_coeffs(&coeffs);

        let mut equalization_filter = Fir::new(coeffs.clone());

        debug!(
            "Training completed: consumed {} symbols, computed {} coefficients, coeffs: {:?}",
            signal_length / self.nsym,
            coeffs.len(),
            coeffs[..10.min(coeffs.len())].to_vec()
        );

        // Pre-load equalization filter with the signal (+lookahead)
        let mut equalized = equalization_filter.process(signal);
        // equalized = equalized[prefix+lookahead:-postfix+lookahead]
        equalized = equalized
            [prefix + lookahead..equalized.len() - postfix + lookahead]
            .to_vec();

        // self.verify_training(&equalized, &train_symbols)?;

        Ok(Fir::new(coeffs))
    }

    fn plot_coeffs(&self, coeffs: &[f64]) {
        use plotly::Plot;
        use plotly::layout::Layout;
        use plotly::scatter::Scatter;

        let x: Vec<usize> = (0..coeffs.len()).collect();
        let y: Vec<f64> = coeffs.to_vec();

        let trace = Scatter::new(x, y).name("Filter Coefficients");

        let mut plot = Plot::new();
        plot.add_trace(trace);

        let layout = Layout::new();
        plot.set_layout(layout);

        plot.write_html("/tmp/coeffs.html");
        info!("Coefficients plot saved to /tmp/coeffs.html");
    }

    pub fn run<I, W>(
        &mut self,
        sampler: &mut Sampler<I>,
        gain: f64,
        mut output: W,
    ) -> Result<(), String>
    where
        I: Iterator<Item = f64>,
        W: Write,
    {
        debug!("Receiving");

        // Step 1: Verify prefix - create a temporary Demux for prefix checking
        {
            let mut symbols = Demux::new(
                |nsym| sampler.take(nsym),
                &self.omegas,
                self.nsym,
                gain,
            );
            self.prefix(&mut symbols, gain)?;
        }
        // symbols is dropped here, releasing the borrow

        // Step 2: Train equalization filter
        let _fir = self.train(sampler, 10, 10)?;
        info!("Equalization filter trained");

        // TODO: Apply FIR filter to equalize the signal
        // For now, we'll skip the equalization and proceed directly to demodulation

        // Step 3: Collect remaining data symbols - create a new Demux for data collection
        let mut symbols =
            Demux::new(|nsym| sampler.take(nsym), &self.omegas, self.nsym, gain);
        let mut data_symbols: Vec<Complex64> = Vec::new();
        while let Some(row) = symbols.next() {
            let carrier_symbol = row
                .get(self.carrier_index)
                .copied()
                .or_else(|| row.first().copied())
                .unwrap_or(Complex64::new(0.0, 0.0));
            data_symbols.push(carrier_symbol);
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

        // Decode symbols to bits
        let bit_tuples = self
            .modem
            .decode(data_symbols);
        let bits_iter = bit_tuples
            .into_iter()
            .flat_map(|tuple| tuple.into_iter());

        // Decode frames from bitstream
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

        // Summary statistics
        let received_kb = self.output_size as f64 / 1e3;
        eprintln!("Received {:.3} kB", received_kb);
        Ok(())
    }
}
