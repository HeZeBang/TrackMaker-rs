// CRC8 implementation for frame integrity checking
// Polynomial: x^8 + x^2 + x + 1 (0x07)

const CRC8_POLYNOMIAL: u8 = 0x07;

/// Calculate CRC8 checksum for given data
pub fn calculate_crc8(data: &[u8]) -> u8 {
    let mut crc: u8 = 0x00;

    for &byte in data {
        crc ^= byte;
        for _ in 0..8 {
            if (crc & 0x80) != 0 {
                crc = (crc << 1) ^ CRC8_POLYNOMIAL;
            } else {
                crc <<= 1;
            }
        }
    }

    crc
}

/// Verify CRC8 checksum
pub fn verify_crc8(data: &[u8], expected_crc: u8) -> bool {
    calculate_crc8(data) == expected_crc
}

/// Convert byte to bit array (MSB first)
pub fn byte_to_bits(byte: u8) -> [u8; 8] {
    let mut bits = [0u8; 8];
    for i in 0..8 {
        bits[i] = ((byte >> (7 - i)) & 1) as u8;
    }
    bits
}

/// Convert bit array to byte (MSB first)
pub fn bits_to_byte(bits: &[u8]) -> u8 {
    let mut byte = 0u8;
    for (i, &bit) in bits
        .iter()
        .enumerate()
        .take(8)
    {
        if bit != 0 {
            byte |= 1 << (7 - i);
        }
    }
    byte
}

/// Convert bytes to bit vector
pub fn bytes_to_bits(bytes: &[u8]) -> Vec<u8> {
    let mut bits = Vec::with_capacity(bytes.len() * 8);
    for &byte in bytes {
        bits.extend_from_slice(&byte_to_bits(byte));
    }
    bits
}

/// Convert bit vector to bytes
pub fn bits_to_bytes(bits: &[u8]) -> Vec<u8> {
    let num_bytes = (bits.len() + 7) / 8;
    let mut bytes = Vec::with_capacity(num_bytes);

    for i in 0..num_bytes {
        let start = i * 8;
        let end = (start + 8).min(bits.len());
        let bit_slice = &bits[start..end];
        bytes.push(bits_to_byte(bit_slice));
    }

    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crc8() {
        let data = b"Hello, World!";
        let crc = calculate_crc8(data);
        assert!(verify_crc8(data, crc));

        // Verify that modified data fails
        let mut modified = data.to_vec();
        modified[0] = b'h';
        assert!(!verify_crc8(&modified, crc));
    }

    #[test]
    fn test_bit_conversion() {
        let byte = 0b10110011;
        let bits = byte_to_bits(byte);
        assert_eq!(bits, [1, 0, 1, 1, 0, 0, 1, 1]);
        assert_eq!(bits_to_byte(&bits), byte);
    }

    #[test]
    fn test_bytes_bits_conversion() {
        let bytes = vec![0xAB, 0xCD, 0xEF];
        let bits = bytes_to_bits(&bytes);
        assert_eq!(bits.len(), 24);
        let recovered = bits_to_bytes(&bits);
        assert_eq!(bytes, recovered);
    }
}
