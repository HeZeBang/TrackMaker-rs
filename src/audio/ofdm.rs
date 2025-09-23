use rustfft::{FftPlanner, num_complex::Complex};
use std::f32::consts::PI;

/// Simple OFDM implementation tailored for audio experiments.
/// Not a production-grade modem; focused on clarity and integration
/// with the existing project test flow.
#[derive(Clone)]
pub struct OfdmParams {
    pub fs: usize,
    pub n_fft: usize,
    pub cp_len: usize,
    pub k_min: usize,
    pub k_max: usize,
    pub data_bins: Vec<usize>,   // positive-frequency bins used for data
    pub pilot_bins: Vec<usize>,  // subset of data bins used as pilots
}

impl OfdmParams {
    pub fn new(fs: usize) -> Self {
        let n_fft = 256usize;
        let cp_len = 64usize;

        // Subcarrier spacing
        let df = fs as f32 / n_fft as f32;

        // Use approximately 3kHz - 17kHz
        let k_min = ((3000.0 / df).ceil() as usize).max(1);
        let k_max = ((17000.0 / df).floor() as usize).min(n_fft / 2 - 1);

        // Build data bins (positive frequency side)
        let mut data_bins: Vec<usize> = (k_min..=k_max).collect();

        // Reserve every 8th bin as pilot
        let mut pilot_bins = Vec::new();
        for (i, &k) in data_bins.iter().enumerate() {
            if i % 8 == 0 {
                pilot_bins.push(k);
            }
        }

        // Remove pilots from data_bins (data_bins keeps carrying all possible data bins,
        // but higher-level code will place pilots explicitly)
        // We'll treat data_bins as full set and just be careful when filling symbols.

        OfdmParams {
            fs,
            n_fft,
            cp_len,
            k_min,
            k_max,
            data_bins,
            pilot_bins,
        }
    }

    /// Number of usable data subcarriers (excluding pilots)
    pub fn usable_data_subcarriers(&self) -> usize {
        self.data_bins.len() - self.pilot_bins.len()
    }
}

/// Map 2 bits -> QPSK symbol (normalized)
fn qpsk_map(b0: u8, b1: u8) -> Complex<f32> {
    // Gray mapping: 00 -> (1,1), 01 -> (-1,1), 11 -> (-1,-1), 10 -> (1,-1)
    let (re, im) = match ((b0 & 1), (b1 & 1)) {
        (0, 0) => (1.0f32, 1.0f32),
        (0, 1) => (-1.0, 1.0),
        (1, 1) => (-1.0, -1.0),
        (1, 0) => (1.0, -1.0),
        _ => (1.0, 1.0),
    };
    // normalize
    Complex::new(re / std::f32::consts::SQRT_2, im / std::f32::consts::SQRT_2)
}

/// Simple hard-decision QPSK demapper
fn qpsk_demap(sym: Complex<f32>) -> (u8, u8) {
    let re = sym.re;
    let im = sym.im;
    let b0 = if re < 0.0 { 1u8 } else { 0u8 };
    let b1 = if im < 0.0 { 1u8 } else { 0u8 };
    (b0, b1)
}

/// IFFT wrapper using rustfft
fn ifft_time_domain(freq: &mut [Complex<f32>]) -> Vec<f32> {
    let n = freq.len();
    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_inverse(n);
    // rustfft expects time-in-place, so operate on a vector copy
    let mut buffer = freq.to_vec();
    fft.process(&mut buffer);
    // rustfft does not normalize inverse; divide by n
    buffer.iter().map(|c| c.re / n as f32).collect()
}

/// FFT wrapper returning complex bins
fn fft_freq_domain(time: &[f32]) -> Vec<Complex<f32>> {
    let n = time.len();
    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(n);
    let mut buffer: Vec<Complex<f32>> = time.iter().map(|&x| Complex::new(x, 0.0)).collect();
    fft.process(&mut buffer);
    buffer
}

/// Build a real-valued OFDM time-domain symbol (with CP) from complex-valued positive-side bins.
/// data_symbols should provide symbols for the usable data bins in order.
/// pilot_symbol is used for pilot bins.
/// Returns time-domain samples (with CP prepended).
pub fn build_ofdm_symbol(params: &OfdmParams, data_symbols: &[Complex<f32>], pilot_symbol: Complex<f32>) -> Vec<f32> {
    let n = params.n_fft;
    let mut freq: Vec<Complex<f32>> = vec![Complex::new(0.0, 0.0); n];

    // Fill positive-frequency bins k_min..=k_max
    // We'll iterate data_bins and pick from data_symbols, skipping pilot positions.
    let mut data_iter = data_symbols.iter();
    for &k in params.data_bins.iter() {
        if params.pilot_bins.contains(&k) {
            freq[k] = pilot_symbol;
        } else {
            // take next data symbol or 0 if exhausted
            let sym = data_iter.next().copied().unwrap_or(Complex::new(0.0, 0.0));
            freq[k] = sym;
        }
    }

    // Enforce Hermitian symmetry: freq[n-k] = conj(freq[k]) for k=1..n/2-1
    for k in 1..(n / 2) {
        freq[n - k] = freq[k].conj();
    }
    // DC and Nyquist left as zero (k=0 and k=n/2)

    // IFFT -> time domain real samples
    let mut time = ifft_time_domain(&mut freq);

    // Add cyclic prefix
    let cp = params.cp_len.min(time.len());
    let mut symbol_with_cp = Vec::with_capacity(cp + time.len());
    symbol_with_cp.extend_from_slice(&time[time.len() - cp..]);
    symbol_with_cp.append(&mut time);

    // Normalize to avoid clipping
    let peak = symbol_with_cp.iter().copied().fold(0.0f32, |a, b| a.max(b.abs()));
    if peak > 0.0 {
        let scale = 0.8f32 / peak;
        for s in symbol_with_cp.iter_mut() {
            *s *= scale;
        }
    }

    symbol_with_cp
}

/// Generate a linear chirp preamble same style as previous main.rs (duration ~440 samples at caller's Fs assumption).
pub fn generate_chirp_preamble(fs: usize) -> Vec<f32> {
    // We'll generate 440 samples like original code assumed 48kHz. To be safe, scale to runtime fs proportionally.
    let base_len = 440usize;
    let factor = (fs as f32 / 48000.0).max(0.5);
    let len = ((base_len as f32 * factor).round() as usize).max(220);

    // f sweep from 2k to 10k and back (split)
    let half = len / 2;
    let mut fp = Vec::with_capacity(len);
    for i in 0..half {
        fp.push(2000.0 + (8000.0 * i as f32 / ((half - 1) as f32)));
    }
    for i in 0..(len - half) {
        fp.push(10000.0 - (8000.0 * i as f32 / ((len - half - 1) as f32).max(1.0)));
    }

    // integrate to make phase and produce sine
    let mut omega = 0.0f32;
    let mut preamble = Vec::with_capacity(len);
    for i in 0..len {
        let t = i as f32 / fs as f32;
        if i == 0 {
            preamble.push((2.0 * PI * fp[0] * t).sin());
        } else {
            let dt = (i as f32 - (i as f32 - 1.0)) / fs as f32;
            omega += PI * (fp[i] + fp[i.saturating_sub(1)]) * dt;
            preamble.push(omega.sin());
        }
    }
    preamble
}

/// Build frames for test mode (encode payload bits into OFDM symbols)
/// Returns concatenated time-domain samples for the entire transmission.
pub fn build_ofdm_frame_stream(params: &OfdmParams, payload_bits: &[u8], frame_id_start: u8) -> Vec<f32> {
    let mut out = Vec::new();

    // Preamble
    let preamble = generate_chirp_preamble(params.fs);
    out.extend_from_slice(&preamble);

    // small guard
    out.extend_from_slice(&vec![0.0f32; 64]);

    // Training symbol (all ones BPSK)
    // Prepare training data symbols (fill data_bins excluding pilots)
    let mut training_data = Vec::new();
    let usable = params.usable_data_subcarriers();
    for _ in 0..usable {
        // use +1 (normalized) as training symbol
        training_data.push(Complex::new(1.0 / std::f32::consts::SQRT_2, 1.0 / std::f32::consts::SQRT_2));
    }
    let pilot_sym = Complex::new(1.0 / std::f32::consts::SQRT_2, 1.0 / std::f32::consts::SQRT_2);
    let t_sym = build_ofdm_symbol(params, &training_data, pilot_sym);
    out.extend_from_slice(&t_sym);

    // Header symbol: encode payload length (16 bits) + frame id (8 bits) + simple CRC8 placeholder (8 bits) into QPSK across subcarriers
    let payload_bits_len = payload_bits.len() as u16;
    let mut header_bits = Vec::new();
    header_bits.extend_from_slice(&payload_bits_len.to_be_bytes());
    header_bits.push(frame_id_start);
    header_bits.push(0u8); // placeholder CRC8

    // Pad header_bits to fill data capacity (2 bits per subcarrier)
    let bits_per_symbol = 2 * params.usable_data_subcarriers();
    while header_bits.len() * 8 < bits_per_symbol {
        header_bits.push(0u8);
    }

    // Flatten header_bits into bit vector
    let mut header_bitvec = Vec::new();
    for &b in header_bits.iter() {
        for i in 0..8 {
            header_bitvec.push((b >> (7 - i)) & 1);
        }
    }
    header_bitvec.truncate(bits_per_symbol);

    // Map header bits to QPSK symbols
    let mut header_syms = Vec::new();
    for i in (0..header_bitvec.len()).step_by(2) {
        let b0 = header_bitvec[i];
        let b1 = header_bitvec.get(i + 1).copied().unwrap_or(0);
        header_syms.push(qpsk_map(b0, b1));
    }
    let hdr_sym = build_ofdm_symbol(params, &header_syms, pilot_sym);
    out.extend_from_slice(&hdr_sym);

    // Payload: map payload_bits into OFDM symbols
    let mut bit_cursor = 0usize;
    let total_bits = payload_bits.len();
    let bits_per_ofdm = 2 * params.usable_data_subcarriers();

    while bit_cursor < total_bits {
        // collect bits for one OFDM symbol
        let mut sym_bits = Vec::new();
        for _ in 0..bits_per_ofdm {
            if bit_cursor < total_bits {
                sym_bits.push(payload_bits[bit_cursor]);
                bit_cursor += 1;
            } else {
                sym_bits.push(0u8);
            }
        }
        // map to QPSK symbols
        let mut data_syms = Vec::new();
        for i in (0..sym_bits.len()).step_by(2) {
            let b0 = sym_bits[i];
            let b1 = sym_bits.get(i + 1).copied().unwrap_or(0);
            data_syms.push(qpsk_map(b0, b1));
        }
        let data_sym_td = build_ofdm_symbol(params, &data_syms, pilot_sym);
        out.extend_from_slice(&data_sym_td);
    }

    // small tail gap
    out.extend_from_slice(&vec![0.0f32; 128]);

    out
}

/// Simple receiver for test mode which expects the same stream as build_ofdm_frame_stream.
/// This is intentionally straightforward: detect preamble by correlation, then parse training, header, and payload.
/// Returns decoded payload bits.
pub fn decode_ofdm_stream(params: &OfdmParams, rx: &[f32]) -> Vec<u8> {
    // 1) detect preamble using simple normalized correlation
    let preamble = generate_chirp_preamble(params.fs);
    let pre_len = preamble.len();
    let mut best_idx = None;
    let mut best_val = 0.0f32;

    if rx.len() < pre_len {
        return vec![];
    }

    for i in 0..=(rx.len() - pre_len) {
        let mut dot = 0.0f32;
        let mut eng_rx = 0.0f32;
        let mut eng_p = 0.0f32;
        for j in 0..pre_len {
            dot += rx[i + j] * preamble[j];
            eng_rx += rx[i + j] * rx[i + j];
            eng_p += preamble[j] * preamble[j];
        }
        let denom = (eng_rx * eng_p).sqrt().max(1e-9);
        let corr = dot / denom;
        if corr > best_val {
            best_val = corr;
            best_idx = Some(i);
        }
    }

    let start = match best_idx {
        Some(idx) => idx + pre_len + 64, // move after preamble + guard
        None => return vec![],
    };

    // Extract training symbol (one OFDM symbol with CP)
    let ofdm_symbol_len = params.n_fft + params.cp_len;
    if start + ofdm_symbol_len > rx.len() {
        return vec![];
    }
    let train_td = &rx[start..start + ofdm_symbol_len];
    // remove CP
    let train_time = &train_td[params.cp_len..params.cp_len + params.n_fft];
    let train_fft = fft_freq_domain(train_time);

    // Build reference training frequency domain (we used +1+j / sqrt(2) on data bins)
    let mut ref_freq: Vec<Complex<f32>> = vec![Complex::new(0.0, 0.0); params.n_fft];
    for &k in params.data_bins.iter() {
        if params.pilot_bins.contains(&k) {
            ref_freq[k] = Complex::new(1.0 / std::f32::consts::SQRT_2, 1.0 / std::f32::consts::SQRT_2);
        } else {
            ref_freq[k] = Complex::new(1.0 / std::f32::consts::SQRT_2, 1.0 / std::f32::consts::SQRT_2);
        }
    }
    for k in 1..(params.n_fft / 2) {
        ref_freq[params.n_fft - k] = ref_freq[k].conj();
    }

    // Estimate channel H = Y / X on data bins (avoid division by zero)
    let mut H: Vec<Complex<f32>> = vec![Complex::new(1.0, 0.0); params.n_fft];
    for &k in params.data_bins.iter() {
        let x = ref_freq[k];
        let y = train_fft[k];
        if x.norm() > 1e-6 {
            H[k] = y / x;
            // mirror
            H[params.n_fft - k] = H[k].conj();
        }
    }

    // Move pointer to header symbol
    let hdr_start = start + ofdm_symbol_len;
    if hdr_start + ofdm_symbol_len > rx.len() {
        return vec![];
    }
    let hdr_td = &rx[hdr_start..hdr_start + ofdm_symbol_len];
    let hdr_time = &hdr_td[params.cp_len..params.cp_len + params.n_fft];
    let hdr_fft = fft_freq_domain(hdr_time);

    // Equalize header bins and demap QPSK
    let mut header_bits = Vec::new();
    for &k in params.data_bins.iter() {
        if params.pilot_bins.contains(&k) {
            continue;
        }
        let y = hdr_fft[k];
        let x_est = y / H[k];
        let (b0, b1) = qpsk_demap(x_est);
        header_bits.push(b0);
        header_bits.push(b1);
    }

    // Recover header fields (first 16 bits payload length, next 8 bits frame id)
    let mut header_bytes = Vec::new();
    for byte_i in 0..(header_bits.len() / 8) {
        let mut byte = 0u8;
        for bit_i in 0..8 {
            let bit = header_bits[byte_i * 8 + bit_i];
            byte = (byte << 1) | (bit & 1);
        }
        header_bytes.push(byte);
    }
    if header_bytes.len() < 3 {
        return vec![];
    }
    let payload_len = u16::from_be_bytes([header_bytes[0], header_bytes[1]]) as usize;
    // let frame_id = header_bytes[2];

    // Move pointer to first payload symbol
    let payload_start = hdr_start + ofdm_symbol_len;
    let mut decoded_bits = Vec::new();

    let bits_per_sym = 2 * params.usable_data_subcarriers();
    let mut sym_idx = 0usize;
    loop {
        let sym_pos = payload_start + sym_idx * ofdm_symbol_len;
        if sym_pos + ofdm_symbol_len > rx.len() {
            break;
        }
        let sym_td = &rx[sym_pos..sym_pos + ofdm_symbol_len];
        let sym_time = &sym_td[params.cp_len..params.cp_len + params.n_fft];
        let sym_fft = fft_freq_domain(sym_time);

        // Equalize and demap
        for &k in params.data_bins.iter() {
            if params.pilot_bins.contains(&k) {
                continue;
            }
            let y = sym_fft[k];
            let x_est = y / H[k];
            let (b0, b1) = qpsk_demap(x_est);
            decoded_bits.push(b0);
            decoded_bits.push(b1);
            if decoded_bits.len() >= payload_len {
                break;
            }
        }

        if decoded_bits.len() >= payload_len {
            break;
        }
        sym_idx += 1;
    }

    // Truncate to payload_len bits
    decoded_bits.truncate(payload_len);

    decoded_bits
}
