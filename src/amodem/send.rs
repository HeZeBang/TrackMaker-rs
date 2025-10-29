use crate::amodem::{
    common::{dumps, iterate},
    config::Configuration,
    dsp::Modem,
    equalizer::{Equalizer, SILENCE_LENGTH, get_prefix},
    framing,
};
use num_complex::Complex64;
use std::io::Write;

pub struct Sender<W: Write> {
    gain: f64,
    offset: usize,
    writer: W,
    modem: Modem,
    carriers: Vec<Vec<Complex64>>,
    pilot: Vec<Complex64>,
    silence: Vec<f64>,
    iters_per_report: usize,
    padding: Vec<bool>,
    equalizer: Equalizer,
    config: Configuration,
}

impl<W: Write> Sender<W> {
    pub fn new(writer: W, config: &Configuration, gain: f64) -> Self {
        let modem = Modem::new(config.symbols.clone());
        let carriers: Vec<Vec<Complex64>> = config.carriers.clone();
        let pilot = config.carriers[config.carrier_index].clone();
        let silence = vec![0.0; SILENCE_LENGTH * config.nsym];
        let iters_per_report = config.baud;
        let padding = vec![false; config.bits_per_baud];
        let equalizer = Equalizer::new(config);

        Self {
            gain,
            offset: 0,
            writer,
            modem,
            carriers,
            pilot,
            silence,
            iters_per_report,
            padding,
            equalizer,
            config: config.clone(),
        }
    }

    pub fn write_samples(&mut self, samples: &[f64]) -> std::io::Result<()> {
        let scaled_samples: Vec<f64> = samples
            .iter()
            .map(|&s| s * self.gain)
            .collect();
        let data = dumps(&scaled_samples);
        self.writer.write_all(&data)?;
        self.offset += samples.len();
        Ok(())
    }

    pub fn write_complex(
        &mut self,
        samples: &[Complex64],
    ) -> std::io::Result<()> {
        // Python version: sym.real * scaling, matching common.dumps()
        let real_samples: Vec<f64> = samples
            .iter()
            .map(|c| c.re * self.gain)
            .collect();
        let data = dumps(&real_samples);
        self.writer.write_all(&data)?;
        self.offset += samples.len();
        Ok(())
    }

    pub fn start(&mut self) -> std::io::Result<()> {
        // Write prefix with pilot tone - matching Python: self.write(self.pilot * value)
        let prefix = get_prefix();
        for &value in &prefix {
            let pilot_signal: Vec<Complex64> = self
                .pilot
                .iter()
                .map(|&c| c * value)
                .collect();
            self.write_complex(&pilot_signal)?;
        }

        // Generate and write training symbols
        let symbols = self
            .equalizer
            .train_symbols(crate::amodem::equalizer::EQUALIZER_LENGTH);
        let signal = self
            .equalizer
            .modulator(&symbols);

        let silence = self.silence.clone();
        self.write_samples(&silence)?;
        self.write_samples(&signal)?;
        self.write_samples(&silence)?;

        Ok(())
    }

    pub fn modulate(
        &mut self,
        bits: impl Iterator<Item = bool>,
    ) -> std::io::Result<()> {
        let all_bits: Vec<bool> = bits
            .chain(self.padding.iter().copied())
            .collect();
        let symbols = self
            .modem
            .encode(all_bits.into_iter());
        let nfreq = self.carriers.len();
        let carrier_len = self.carriers[0].len();

        for (i, symbol_chunk) in
            iterate(symbols.into_iter(), nfreq, None).enumerate()
        {
            // Pad to nfreq if necessary
            let mut symbols = symbol_chunk;
            while symbols.len() < nfreq {
                symbols.push(Complex64::new(0.0, 0.0));
            }

            // Compute signal as dot product of symbols and carriers (normalized)
            let mut signal = vec![Complex64::new(0.0, 0.0); carrier_len];
            for (j, &symbol) in symbols.iter().enumerate() {
                if j < nfreq {
                    for (k, &carrier) in self.carriers[j]
                        .iter()
                        .enumerate()
                    {
                        signal[k] += symbol * carrier / nfreq as f64;
                    }
                }
            }

            // Convert to real signal
            let real_signal: Vec<f64> = signal
                .iter()
                .map(|c| c.re)
                .collect();

            self.write_samples(&real_signal)?;

            if (i + 1) % self.iters_per_report == 0 {
                let total_bits = (i + 1) * nfreq * self.modem.bits_per_symbol();
                eprintln!("Sent {:10.3} kB", total_bits as f64 / 8e3);
            }
        }

        Ok(())
    }

    pub fn get_offset(&self) -> usize {
        self.offset
    }
}

pub fn send<R: std::io::Read, W: Write>(
    config: &Configuration,
    mut src: R,
    dst: W,
    gain: f64,
    extra_silence: f64,
) -> std::io::Result<()> {
    send_with_reed_solomon(config, src, dst, gain, extra_silence, false, 8)
}

pub fn send_with_reed_solomon<R: std::io::Read, W: Write>(
    config: &Configuration,
    mut src: R,
    dst: W,
    gain: f64,
    extra_silence: f64,
    use_reed_solomon: bool,
    ecc_len: usize,
) -> std::io::Result<()> {
    let mut sender = Sender::new(dst, config, gain);

    // Pre-padding with silence
    let silence_samples =
        (config.fs * (config.silence_start + extra_silence)) as usize;
    let silence = vec![0.0; silence_samples];
    sender.write_samples(&silence)?;

    sender.start()?;

    let training_duration = sender.get_offset();
    eprintln!(
        "Sending {:.3} seconds of training audio",
        training_duration as f64 / config.fs
    );

    // Read input data
    let mut data = Vec::new();
    src.read_to_end(&mut data)?;

    // Encode data to bits
    let bits = if use_reed_solomon {
        framing::encode_with_reed_solomon(&data, ecc_len)
    } else {
        framing::encode(&data)
    };

    eprintln!("Starting modulation");
    sender.modulate(bits.into_iter())?;

    let data_duration = sender.get_offset() - training_duration;
    eprintln!(
        "Sent {:.3} kB @ {:.3} seconds",
        data.len() as f64 / 1e3,
        data_duration as f64 / config.fs
    );

    // Post-padding with silence
    let silence_samples = (config.fs * config.silence_stop) as usize;
    let silence = vec![0.0; silence_samples];
    sender.write_samples(&silence)?;

    Ok(())
}
