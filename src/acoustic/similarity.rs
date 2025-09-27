use rayon::prelude::*;
use rustfft::{FftPlanner, num_complex::Complex};
use std::iter::repeat;

fn pad_signal(signal: &[f32], size: usize) -> Vec<Complex<f32>> {
    let mut padded: Vec<Complex<f32>> = signal
        .iter()
        .map(|&x| Complex::new(x, 0.0))
        .collect();
    padded.extend(repeat(Complex::new(0.0, 0.0)).take(size - signal.len()));
    padded
}

fn fft_convolve(signal1: &[f32], signal2: &[f32]) -> Vec<f32> {
    let n = signal1.len() + signal2.len() - 1;
    let padded_signal1 = pad_signal(signal1, n);
    let padded_signal2 = pad_signal(signal2, n);

    let mut planner = FftPlanner::new();
    let fft = planner.plan_fft_forward(n);
    let ifft = planner.plan_fft_inverse(n);

    let mut signal1_fft = padded_signal1;
    let mut signal2_fft = padded_signal2;
    fft.process(&mut signal1_fft);
    fft.process(&mut signal2_fft);

    let mut result_fft: Vec<Complex<f32>> = signal1_fft
        .par_iter()
        .zip(signal2_fft.par_iter())
        .map(|(&a, &b)| a * b)
        .collect();

    ifft.process(&mut result_fft);
    result_fft
        .iter()
        .map(|c| c.re / n as f32)
        .collect()
}

fn sliding_l2_norm(signal: &[f32], window_size: usize) -> Vec<f32> {
    if window_size == 0 || signal.len() < window_size {
        return Vec::new();
    }

    let mut l2_norms = Vec::with_capacity(signal.len() - window_size + 1);
    let mut current_l2_norm = signal[0..window_size]
        .iter()
        .map(|e| e * e)
        .sum::<f32>();
    l2_norms.push(current_l2_norm);

    for i in 0..signal.len() - window_size {
        let j = i + window_size;
        current_l2_norm -= signal[i] * signal[i];
        current_l2_norm += signal[j] * signal[j];
        l2_norms.push(current_l2_norm);
    }
    l2_norms
}

pub fn similarity(reference: &[f32], source: &[f32]) -> Vec<f32> {
    if reference.is_empty() || source.len() < reference.len() {
        return Vec::new();
    }

    let reference_reversed: Vec<f32> = reference
        .iter()
        .rev()
        .copied()
        .collect();
    let result = fft_convolve(&reference_reversed, source);
    let start = reference.len() - 1;
    result[start..start + source.len() - reference.len() + 1].to_vec()
}

pub fn cosine_similarity(reference: &[f32], source: &[f32]) -> Vec<f32> {
    if reference.is_empty() || source.len() < reference.len() {
        return Vec::new();
    }

    let similarity = similarity(reference, source);
    let reference_l2_norm: f32 = reference
        .iter()
        .map(|&x| x * x)
        .sum();
    let source_l2_norms = sliding_l2_norm(source, reference.len());

    similarity
        .into_iter()
        .zip(source_l2_norms.into_iter())
        .map(|(mut value, source_norm)| {
            if value.abs() < 1e-5 * reference.len() as f32 {
                0.0
            } else {
                value /= (reference_l2_norm * source_norm).sqrt();
                value
            }
        })
        .collect()
}
