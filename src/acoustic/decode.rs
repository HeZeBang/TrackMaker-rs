use crate::acoustic::{
    b4b5::decode_5b_to_4bu8,
    config::{ECC_LEN, FRAME_LEN, HEAD_LEN, NUM_SAMPLES},
};
use reed_solomon::Decoder;

pub fn decode(buffer: &[f32]) -> (Vec<u8>, bool) {
    let mut right = true;
    let dec = Decoder::new(ECC_LEN);
    let mut data: Vec<u8> = vec![0; FRAME_LEN];
    let mut data_5b: Vec<u8> = vec![0; (FRAME_LEN + ECC_LEN) * 10];

    for i in 0..(buffer.len() / NUM_SAMPLES) {
        let power_sum: f32 = (1..NUM_SAMPLES - 1)
            .map(|j| buffer[i * NUM_SAMPLES + j])
            .sum();
        data_5b[i] = if power_sum > 0.0 { 1 } else { 0 };
    }

    let mut data_4b_u8 = decode_5b_to_4bu8(&data_5b);

    if ECC_LEN != 0 {
        let known_erasures = [];
        let recovered = match dec.correct(&mut data_4b_u8, Some(&known_erasures))
        {
            Ok(buffer) => buffer.to_vec(),
            Err(e) => {
                eprintln!("Error during Reed-Solomon decoding: {:?}", e);
                right = false;
                data_4b_u8[..FRAME_LEN].to_vec()
            }
        };
        data[..].copy_from_slice(&recovered[..FRAME_LEN]);
    } else {
        data[..].copy_from_slice(&data_4b_u8[..FRAME_LEN]);
    }

    (data, right)
}

pub fn head_decode(buffer: &[f32]) -> Vec<u8> {
    let mut data_5b: Vec<u8> = vec![0; HEAD_LEN * 10];
    for i in 0..(buffer.len() / NUM_SAMPLES) {
        let power_sum: f32 = (1..NUM_SAMPLES - 1)
            .map(|j| buffer[i * NUM_SAMPLES + j])
            .sum();
        data_5b[i] = if power_sum > 0.0 { 1 } else { 0 };
    }
    decode_5b_to_4bu8(&data_5b)
}
