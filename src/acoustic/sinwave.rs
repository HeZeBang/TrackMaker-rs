use std::f32::consts::PI;

pub fn generate_sine_wave(
    frequency: f32,
    sample_rate: f32,
    duration: f32,
) -> Vec<f32> {
    let total_samples = (sample_rate * duration) as usize;
    (0..total_samples)
        .map(|i| {
            let t = i as f32 / sample_rate;
            (2.0 * PI * frequency * t).sin()
        })
        .collect()
}
