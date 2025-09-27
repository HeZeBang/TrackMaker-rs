use crate::acoustic::{
    b4b5::encode_u8_to_repeated_5b,
    config::{self, ECC_LEN, FRAME_LEN, HEAD_LEN},
};
use reed_solomon::Encoder;

pub fn encode(
    mut input: Vec<u8>,
    preamble: &[f32],
    frame_index: usize,
    frame_type: f32,
) -> Vec<f32> {
    if input.len() < FRAME_LEN {
        input.resize(FRAME_LEN, 0);
    }

    let mut output_track = Vec::new();
    let enc = Encoder::new(ECC_LEN);
    let frame_len = if (frame_type - config::DATA_TYPE).abs() < f32::EPSILON {
        FRAME_LEN
    } else {
        0
    };
    let mut frame_u8: Vec<u8> = vec![0; HEAD_LEN + frame_len + ECC_LEN];
    frame_u8[0] = config::DEVICE_ID as u8;
    frame_u8[1] = config::SERVER_ID as u8;
    frame_u8[2] = frame_type as u8;
    frame_u8[3] = frame_index as u8;

    for i in 0..frame_len {
        frame_u8[HEAD_LEN + i] = input[i];
    }

    if ECC_LEN != 0 {
        let encoded_data =
            enc.encode(&frame_u8[HEAD_LEN..HEAD_LEN + frame_len].to_vec());
        for i in HEAD_LEN + frame_len..encoded_data.len() {
            frame_u8[i] = encoded_data[i];
        }
    }

    let frame_wave = encode_u8_to_repeated_5b(&frame_u8);
    output_track.extend_from_slice(preamble);
    output_track.extend_from_slice(&frame_wave);

    output_track
}
