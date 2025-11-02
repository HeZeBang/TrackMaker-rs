use crate::amodem::{
    config::Configuration,
    dsp::{Demux, Fir, Modem, rms, rms_2d},
    equalizer::{
        EQUALIZER_LENGTH, Equalizer, SILENCE_LENGTH, get_prefix, train,
    },
    framing,
    sampling::Sampler,
};
use num_complex::Complex64;
use std::collections::HashMap;
use std::io::Write;
use tracing::{debug, info, warn};

/// Iterator that transposes multiple bit streams (similar to Python's zip(*streams))
struct BitstreamTransposer {
    streams: Vec<Box<dyn Iterator<Item = Vec<bool>>>>,
}

impl BitstreamTransposer {
    fn new(streams: Vec<Box<dyn Iterator<Item = Vec<bool>>>>) -> Self {
        Self { streams }
    }
}

impl Iterator for BitstreamTransposer {
    type Item = Vec<Vec<bool>>;

    fn next(&mut self) -> Option<Self::Item> {
        let mut result = Vec::new();
        let mut any_has_data = false;

        for stream in &mut self.streams {
            if let Some(bits) = stream.next() {
                result.push(bits);
                any_has_data = true;
            } else {
                // If one stream is exhausted, we could either stop or pad with empty
                // For now, let's stop when any stream is exhausted
                return None;
            }
        }

        if any_has_data { Some(result) } else { None }
    }
}

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
            .take(signal_length + lookahead)
            .ok_or_else(|| {
                "Not enough samples from sampler for equalizer training"
                    .to_string()
            })?;

        let mut expected = train_signal.clone();
        expected.extend(vec![0.0; lookahead]);

        let _signal = signal[prefix..signal.len() - postfix].to_vec();

        // Compute filter coefficients using Levinson-Durbin
        let coeffs = train(&_signal, &expected, order, lookahead);

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

        // debug!(
        //     "Equalized signal: {:?}",
        //     equalized[..10.min(equalized.len())].to_vec()
        // );

        self.verify_training(&equalized, &train_symbols)?;

        Ok(Fir::new(coeffs))
    }

    fn verify_training(
        &self,
        equalized: &[f64],
        train_symbols: &[Vec<Complex64>],
    ) -> Result<(), String> {
        // Demodulate equalized signal
        let symbols = self
            .equalizer
            .demodulator(equalized, EQUALIZER_LENGTH);

        // sliced = np.array(symbols).round()
        let sliced: Vec<Vec<num_complex::Complex<f64>>> = symbols
            .iter()
            .map(|row| {
                row.iter()
                    .map(|c| {
                        num_complex::Complex::new(c.re.round(), c.im.round())
                    })
                    .collect()
            })
            .collect();
        // errors = np.array(sliced - train_symbols, dtype=bool)
        let errors = sliced
            .iter()
            .zip(train_symbols.iter())
            .map(|(s_row, t_row)| {
                s_row
                    .iter()
                    .zip(t_row.iter())
                    .map(|(s, t)| {
                        let re_err = (s.re as i64 - t.re as i64).abs() > 1;
                        let im_err = (s.im as i64 - t.im as i64).abs() > 1;
                        re_err || im_err
                    })
                    .collect::<Vec<bool>>()
            })
            .collect::<Vec<Vec<bool>>>();
        let error_rate = errors
            .iter()
            .flatten()
            .filter(|&&e| e)
            .count() as f64
            / (errors.len() * errors[0].len()) as f64;

        info!("Error rate: {:.3}%", error_rate * 100.0);

        // Calculate errors: symbols - train_symbols
        let mut error_matrix: Vec<Vec<Complex64>> = Vec::new();
        for (i, symbol_row) in symbols.iter().enumerate() {
            if i >= train_symbols.len() {
                break;
            }
            let mut error_row = Vec::new();
            for (j, &received) in symbol_row.iter().enumerate() {
                if j >= train_symbols[i].len() {
                    break;
                }
                error_row.push(received - train_symbols[i][j]);
            }
            if !error_row.is_empty() {
                error_matrix.push(error_row);
            }
        }

        let noise_rms = rms_2d(&error_matrix);
        let signal_rms = rms_2d(train_symbols);

        // Calculate SNR for each frequency
        for (i, (&signal, &noise)) in signal_rms
            .iter()
            .zip(noise_rms.iter())
            .enumerate()
        {
            if i < self.frequencies.len() {
                let snr_db = if noise > 1e-10 {
                    20.0 * (signal / noise).log10()
                } else {
                    f64::INFINITY
                };
                let freq_khz = self.frequencies[i] / 1000.0;
                debug!("{:5.1} kHz: SNR = {:5.2} dB", freq_khz, snr_db);
            }
        }

        if error_rate > 0.1 {
            return Err(format!(
                "Training verification failed: error rate {:.4}",
                error_rate
            ));
        }

        debug!("Training verified");
        Ok(())
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

    fn bitstream<S>(
        &self,
        symbols: &mut Demux<S>,
    ) -> (
        Box<dyn Iterator<Item = Vec<Vec<bool>>>>,
        Vec<Vec<Complex64>>,
    )
    where
        S: FnMut(usize) -> Option<Vec<f64>>,
    {
        let num_freqs = self.frequencies.len();
        let mut symbol_list: Vec<Vec<Complex64>> = vec![Vec::new(); num_freqs];

        // Collect all symbol rows first (similar to Python's generators)
        while let Some(row) = symbols.next() {
            for (freq_idx, &sym) in row.iter().enumerate() {
                if freq_idx < symbol_list.len() {
                    symbol_list[freq_idx].push(sym);
                }
            }
        }

        // Create iterators for each frequency
        let mut streams: Vec<Box<dyn Iterator<Item = Vec<bool>>>> = Vec::new();

        for freq_idx in 0..num_freqs {
            let freq_symbols = symbol_list[freq_idx].clone();

            // Decode bits for this frequency (without error handler for now)
            let bits_iter = self
                .modem
                .decode(freq_symbols);

            streams.push(Box::new(bits_iter.into_iter()));
        }

        // Create transposed iterator (zip streams)
        let transposed_iter = BitstreamTransposer::new(streams);

        (Box::new(transposed_iter), symbol_list)
    }

    fn update_sampler<I>(
        &self,
        errors: &mut HashMap<usize, Vec<f64>>,
        sampler: &mut Sampler<I>,
    ) where
        I: Iterator<Item = f64>,
    {
        if errors.is_empty() {
            return;
        }

        let mut all_angles = Vec::new();
        for vals in errors.values() {
            for &v in vals {
                if v > 1e-10 {
                    let angle = v.atan2(0.0); // Extract phase
                    all_angles.push(angle);
                }
            }
        }

        if !all_angles.is_empty() {
            let mean_angle =
                all_angles.iter().sum::<f64>() / all_angles.len() as f64;
            let err = mean_angle / (2.0 * std::f64::consts::PI);

            debug!(
                "Sampler update: frequency offset = {:.6}, phase offset = {:.6}",
                err, err
            );

            sampler.adjust_frequency(-self.freq_err_gain * err);
            sampler.adjust_offset(-err);
        }

        // Clear for next batch
        for v in errors.values_mut() {
            v.clear();
        }
    }

    fn report_progress<I>(
        &self,
        noise: &mut HashMap<usize, Vec<f64>>,
        sampler: &Sampler<I>,
        rx_bits: usize,
    ) where
        I: Iterator<Item = f64>,
    {
        if noise.is_empty() {
            return;
        }

        let mut all_noise = Vec::new();
        for vals in noise.values() {
            for &v in vals {
                all_noise.push(v);
            }
        }

        if !all_noise.is_empty() {
            let mean_noise_power = all_noise
                .iter()
                .map(|v| v * v)
                .sum::<f64>()
                / all_noise.len() as f64;
            let snr_db = if mean_noise_power > 1e-10 {
                -10.0 * mean_noise_power.log10()
            } else {
                f64::INFINITY
            };

            let freq_drift_ppm = (1.0 - sampler.get_frequency()) * 1e6;

            debug!(
                "Got {:10.3} kB, SNR: {:5.2} dB, drift: {:+5.2} ppm",
                rx_bits as f64 / 8e3,
                snr_db,
                freq_drift_ppm
            );
        }

        // Clear for next batch
        for v in noise.values_mut() {
            v.clear();
        }
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
        let mut fir = self.train(sampler, 10, 10)?;
        sampler.set_equalizer(move |input| fir.process(input));
        info!("Equalization filter trained");

        // Step 3: Demodulate - create a new Demux after training is complete
        let mut symbols =
            Demux::new(|nsym| sampler.take(nsym), &self.omegas, self.nsym, gain);
        let (stream, _symbol_list) = self.bitstream(&mut symbols);

        // Step 4: Stream decode and process frames with periodic updates
        let frames =
            framing::decode_frames_from_bits(stream.flat_map(|block_of_bits| {
                block_of_bits
                    .into_iter()
                    .flat_map(|bits| bits.into_iter())
            }));

        let mut _rx_bits = 0usize;
        let mut _block_count = 0usize;

        // iterate and write, with periodic sampler and progress updates
        for frame in frames {
            _block_count += 1;
            _rx_bits += frame.len() * 8; // each byte = 8 bits

            // Periodically update sampler (every 100 blocks)
            // Note: We can't update sampler with errors from bitstream in lazy mode
            // if block_count % self.iters_per_update == 0 {
            //     self.update_sampler(&mut errors, sampler);
            // }
            debug!("Content len {:?}, block: {:?}", frame.len(), _block_count);

            // Periodically report progress (every 1000 blocks)
            if _block_count % self.iters_per_report == 0 {
                debug!("Got {:10.3} kB", _rx_bits as f64 / 8e3);
                //     self.report_progress(&mut noise, sampler, rx_bits);
            }

            // Write frame to output
            output
                .write_all(&frame)
                .map_err(|e| e.to_string())?;
            self.output_size += frame.len();
        }

        Ok(())
    }
}
