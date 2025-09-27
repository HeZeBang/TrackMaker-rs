use std::collections::HashMap;

use crate::acoustic::config::NUM_SAMPLES;

fn init_4b5b_table() -> HashMap<u8, u8> {
    let mut table = HashMap::new();
    table.insert(0b0000, 0b11110);
    table.insert(0b0001, 0b01001);
    table.insert(0b0010, 0b10100);
    table.insert(0b0011, 0b10101);
    table.insert(0b0100, 0b01010);
    table.insert(0b0101, 0b01011);
    table.insert(0b0110, 0b01110);
    table.insert(0b0111, 0b01111);
    table.insert(0b1000, 0b10010);
    table.insert(0b1001, 0b10011);
    table.insert(0b1010, 0b10110);
    table.insert(0b1011, 0b10111);
    table.insert(0b1100, 0b11010);
    table.insert(0b1101, 0b11011);
    table.insert(0b1110, 0b11100);
    table.insert(0b1111, 0b11101);
    table
}

/// 使用4B5B编码对u8数组进行编码，并转换为重复比特数组
pub fn encode_u8_to_repeated_5b(data: &[u8]) -> Vec<f32> {
    let table = init_4b5b_table();
    let mut encoded_bits: Vec<f32> = vec![0.0; data.len() * 10 * NUM_SAMPLES];
    let mut index = 0;

    for &byte in data {
        let high_nibble = (byte >> 4) & 0x0F;
        let low_nibble = byte & 0x0F;

        if let Some(&encoded_high) = table.get(&high_nibble) {
            for i in (0..5).rev() {
                let mut bit: f32 = ((encoded_high >> i) & 1) as f32;
                if bit == 0.0 {
                    bit = -1.0;
                }
                for _ in 0..NUM_SAMPLES {
                    encoded_bits[index] = bit;
                    index += 1;
                }
            }
        }

        if let Some(&encoded_low) = table.get(&low_nibble) {
            for i in (0..5).rev() {
                let mut bit: f32 = ((encoded_low >> i) & 1) as f32;
                if bit == 0.0 {
                    bit = -1.0;
                }
                for _ in 0..NUM_SAMPLES {
                    encoded_bits[index] = bit;
                    index += 1;
                }
            }
        }
    }

    encoded_bits
}

fn init_5b4b_decode_table() -> HashMap<u8, u8> {
    let mut table = HashMap::new();
    table.insert(0b11110, 0b0000);
    table.insert(0b01001, 0b0001);
    table.insert(0b10100, 0b0010);
    table.insert(0b10101, 0b0011);
    table.insert(0b01010, 0b0100);
    table.insert(0b01011, 0b0101);
    table.insert(0b01110, 0b0110);
    table.insert(0b01111, 0b0111);
    table.insert(0b10010, 0b1000);
    table.insert(0b10011, 0b1001);
    table.insert(0b10110, 0b1010);
    table.insert(0b10111, 0b1011);
    table.insert(0b11010, 0b1100);
    table.insert(0b11011, 0b1101);
    table.insert(0b11100, 0b1110);
    table.insert(0b11101, 0b1111);
    table
}

/// 将5位编码的数组解码为原始的u8数组
pub fn decode_5b_to_4bu8(bitstream: &[u8]) -> Vec<u8> {
    let decode_table = init_5b4b_decode_table();
    let mut decoded_bytes = Vec::new();
    let mut nibble_buffer = Vec::new();

    if bitstream.len() % 5 != 0 {
        eprintln!("Error: Invalid bitstream length, must be a multiple of 5");
        return decoded_bytes;
    }

    for chunk in bitstream.chunks(5) {
        let mut encoded_5b = 0u8;

        for (i, &bit) in chunk.iter().enumerate() {
            encoded_5b |= (bit as u8) << (4 - i);
        }

        if let Some(&decoded_4b) = decode_table.get(&encoded_5b) {
            nibble_buffer.push(decoded_4b);
        } else {
            eprintln!("Error: Invalid 5b encoding {:?}", encoded_5b);
            nibble_buffer.push(0x00);
        }

        if nibble_buffer.len() == 2 {
            let byte = (nibble_buffer[0] << 4) | nibble_buffer[1];
            decoded_bytes.push(byte);
            nibble_buffer.clear();
        }
    }

    decoded_bytes
}
