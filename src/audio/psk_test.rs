use crate::audio::psk::{PskModulator, PskDemodulator};

#[test]
fn test_psk_modulation_basic() {
    let sample_rate = 48000.0;
    let carrier_freq = 10000.0;
    let symbol_rate = 1000.0;
    
    let modulator = PskModulator::new(sample_rate, carrier_freq, symbol_rate);
    let demodulator = PskDemodulator::new(sample_rate, carrier_freq, symbol_rate);
    
    // Test with simple bit pattern
    let test_bits = vec![1, 0, 1, 1, 0, 0, 1, 0];
    
    // Modulate
    let modulated = modulator.modulate_bpsk(&test_bits);
    
    // Demodulate
    let demodulated = demodulator.demodulate_bpsk(&modulated);
    
    println!("Original:   {:?}", test_bits);
    println!("Demodulated: {:?}", demodulated);
    
    // Should match exactly for clean signal
    assert_eq!(test_bits.len(), demodulated.len());
    
    // Count correct bits
    let correct_bits = test_bits.iter()
        .zip(demodulated.iter())
        .filter(|(a, b)| a == b)
        .count();
    
    let accuracy = correct_bits as f32 / test_bits.len() as f32;
    println!("Accuracy: {:.1}%", accuracy * 100.0);
    
    // Should have high accuracy for clean signal
    assert!(accuracy > 0.8, "Accuracy too low: {:.1}%", accuracy * 100.0);
}

#[test]
fn test_psk_with_noise() {
    use rand::{Rng, SeedableRng};
    
    let sample_rate = 48000.0;
    let carrier_freq = 10000.0;
    let symbol_rate = 1000.0;
    
    let modulator = PskModulator::new(sample_rate, carrier_freq, symbol_rate);
    let demodulator = PskDemodulator::new(sample_rate, carrier_freq, symbol_rate);
    
    // Test with random bit pattern
    let mut rng = rand::rngs::StdRng::from_seed([42u8; 32]);
    let test_bits: Vec<u8> = (0..16).map(|_| rng.gen_range(0..=1)).collect();
    
    // Modulate
    let mut modulated = modulator.modulate_bpsk(&test_bits);
    
    // Add some noise
    let noise_level = 0.1;
    for sample in modulated.iter_mut() {
        *sample += rng.gen_range(-noise_level..noise_level);
    }
    
    // Demodulate
    let demodulated = demodulator.demodulate_bpsk(&modulated);
    
    println!("Original:   {:?}", test_bits);
    println!("Demodulated: {:?}", demodulated);
    
    // Count correct bits
    let correct_bits = test_bits.iter()
        .zip(demodulated.iter())
        .filter(|(a, b)| a == b)
        .count();
    
    let accuracy = correct_bits as f32 / test_bits.len() as f32;
    println!("Accuracy with noise: {:.1}%", accuracy * 100.0);
    
    // Should still have reasonable accuracy with low noise
    assert!(accuracy > 0.6, "Accuracy too low with noise: {:.1}%", accuracy * 100.0);
}
