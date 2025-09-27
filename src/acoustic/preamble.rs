use std::f32::consts::PI;

use crate::acoustic::similarity::cosine_similarity;

fn sinphi(tau: f32, fmin: f32, fmax: f32) -> f32 {
    let inner_expression = if 2.0 * tau <= 1.0 {
        tau * (-tau * fmin + fmin + fmax * tau)
    } else {
        0.5 * (2.0 * (tau - 1.0) * tau * fmin
            + fmin
            + fmax * (-2.0 * (tau - 2.0) * tau - 1.0))
    };

    (2.0 * PI * inner_expression).sin()
}

pub fn create_preamble(f_start: f32, f_end: f32, samples: usize) -> Vec<f32> {
    linear_sample(samples)
        .into_iter()
        .map(|tau: f32| sinphi(tau, f_start, f_end))
        .collect()
}

fn linear_sample(samples: usize) -> Vec<f32> {
    (0..samples)
        .map(|i| i as f32 / samples as f32)
        .collect()
}

pub fn detect_signal(reference: &[f32], source: &[f32]) -> Vec<usize> {
    let similarities = cosine_similarity(reference, source);
    let mut detected_index: Vec<usize> = Vec::new();
    let mut current_max_similarity: f32 = 0.0;
    let mut current_max_index = 0;
    let threshold = 0.7;

    let mut is_probably_preamble = false;

    for i in 0..similarities.len() {
        if similarities[i] > threshold && !is_probably_preamble {
            current_max_index = i;
            current_max_similarity = similarities[i];
            is_probably_preamble = true;
        }
        if similarities[i] < threshold
            && is_probably_preamble
            && i.saturating_sub(current_max_index) > 200
        {
            detected_index.push(current_max_index);
            is_probably_preamble = false;
        }
        if is_probably_preamble && similarities[i] > current_max_similarity {
            current_max_similarity = similarities[i];
            current_max_index = i;
        }
    }

    if is_probably_preamble {
        detected_index.push(current_max_index);
    }

    detected_index
}
