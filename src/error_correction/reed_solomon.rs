use reed_solomon::{Encoder, Decoder};
use tracing::{debug, warn, error};

/// Reed-Solomon error correction encoder
pub struct ReedSolomonEncoder {
    encoder: Encoder,
    ecc_len: usize,
}

/// Reed-Solomon error correction decoder
pub struct ReedSolomonDecoder {
    decoder: Decoder,
    ecc_len: usize,
}

/// Result of error correction operation
#[derive(Debug)]
pub struct ErrorCorrectionResult {
    /// The corrected data
    pub data: Vec<u8>,
    /// Number of errors detected and corrected
    pub errors_corrected: usize,
    /// Whether the correction was successful
    pub success: bool,
}

impl ReedSolomonEncoder {
    /// Create a new Reed-Solomon encoder with specified error correction code length
    /// 
    /// # Arguments
    /// * `ecc_len` - Length of error correction code (typically 8, 16, 32, etc.)
    ///               Higher values provide better error correction but add more overhead
    pub fn new(ecc_len: usize) -> Self {
        let encoder = Encoder::new(ecc_len);
        Self { encoder, ecc_len }
    }
    
    /// Encode data with Reed-Solomon error correction
    /// 
    /// # Arguments
    /// * `data` - Input data to encode
    /// 
    /// # Returns
    /// Encoded data with error correction codes appended
    pub fn encode(&self, data: &[u8]) -> Vec<u8> {
        debug!("Encoding {} bytes with RS({}, {})", data.len(), data.len() + self.ecc_len, data.len());
        
        let encoded = self.encoder.encode(data);
        let mut result = Vec::with_capacity(data.len() + self.ecc_len);
        
        // Copy original data
        result.extend_from_slice(data);
        // Append error correction codes
        result.extend_from_slice(encoded.ecc());
        
        debug!("Encoded to {} bytes (data: {}, ecc: {})", 
               result.len(), data.len(), self.ecc_len);
        result
    }
    
    /// Get the error correction code length
    pub fn ecc_len(&self) -> usize {
        self.ecc_len
    }
    
    /// Calculate the total size after encoding (original data + ECC)
    pub fn encoded_size(&self, data_len: usize) -> usize {
        data_len + self.ecc_len
    }
}

impl ReedSolomonDecoder {
    /// Create a new Reed-Solomon decoder with specified error correction code length
    /// 
    /// # Arguments
    /// * `ecc_len` - Length of error correction code (must match encoder)
    pub fn new(ecc_len: usize) -> Self {
        let decoder = Decoder::new(ecc_len);
        Self { decoder, ecc_len }
    }
    
    /// Decode and correct errors in received data
    /// 
    /// # Arguments
    /// * `encoded_data` - Received data (original data + error correction codes)
    /// * `known_erasures` - Optional slice of known error positions
    /// 
    /// # Returns
    /// Result of error correction operation
    pub fn decode(&self, encoded_data: &[u8], known_erasures: Option<&[u8]>) -> ErrorCorrectionResult {
        if encoded_data.len() < self.ecc_len {
            error!("Encoded data too short: {} bytes, expected at least {}", 
                   encoded_data.len(), self.ecc_len);
            return ErrorCorrectionResult {
                data: Vec::new(),
                errors_corrected: 0,
                success: false,
            };
        }
        
        let data_len = encoded_data.len() - self.ecc_len;
        debug!("Decoding {} bytes (data: {}, ecc: {})", 
               encoded_data.len(), data_len, self.ecc_len);
        
        // Create a mutable copy for correction
        let mut corrupted = encoded_data.to_vec();
        
        // Attempt correction
        match self.decoder.correct(&mut corrupted, known_erasures) {
            Ok(corrected) => {
                let data = corrected.data().to_vec();
                
                // Count errors corrected by comparing original and corrected
                let mut errors_corrected = 0;
                for i in 0..encoded_data.len() {
                    if encoded_data[i] != corrupted[i] {
                        errors_corrected += 1;
                    }
                }
                
                debug!("Successfully corrected {} errors, recovered {} bytes", 
                       errors_corrected, data.len());
                
                ErrorCorrectionResult {
                    data,
                    errors_corrected,
                    success: true,
                }
            }
            Err(e) => {
                error!("Reed-Solomon correction failed: {:?}", e);
                
                // Return original data without ECC as fallback
                let data = encoded_data[..data_len].to_vec();
                warn!("Returning uncorrected data ({} bytes)", data.len());
                
                ErrorCorrectionResult {
                    data,
                    errors_corrected: 0,
                    success: false,
                }
            }
        }
    }
    
    /// Get the error correction code length
    pub fn ecc_len(&self) -> usize {
        self.ecc_len
    }
    
    /// Extract original data length from encoded data length
    pub fn original_data_len(&self, encoded_len: usize) -> Option<usize> {
        if encoded_len >= self.ecc_len {
            Some(encoded_len - self.ecc_len)
        } else {
            None
        }
    }
}

/// Utility functions for common Reed-Solomon operations
pub mod utils {
    use super::*;
    
    /// Encode data with default Reed-Solomon parameters (ECC length = 16)
    pub fn encode_default(data: &[u8]) -> Vec<u8> {
        let encoder = ReedSolomonEncoder::new(16);
        encoder.encode(data)
    }
    
    /// Decode data with default Reed-Solomon parameters (ECC length = 16)
    pub fn decode_default(encoded_data: &[u8]) -> ErrorCorrectionResult {
        let decoder = ReedSolomonDecoder::new(16);
        decoder.decode(encoded_data, None)
    }
    
    /// Test Reed-Solomon encoding and decoding with simulated errors
    pub fn test_correction(data: &[u8], ecc_len: usize, num_errors: usize) -> ErrorCorrectionResult {
        let encoder = ReedSolomonEncoder::new(ecc_len);
        let decoder = ReedSolomonDecoder::new(ecc_len);
        
        // Encode
        let encoded = encoder.encode(data);
        
        // Simulate errors
        let mut corrupted = encoded.clone();
        let max_correctable = ecc_len / 2;
        
        if num_errors <= max_correctable {
            for i in 0..num_errors {
                if i < corrupted.len() {
                    corrupted[i] = corrupted[i].wrapping_add(1); // Introduce errors
                }
            }
        }
        
        // Decode
        decoder.decode(&corrupted, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_basic_encoding_decoding() {
        let data = b"Hello, Reed-Solomon!";
        let ecc_len = 8;
        
        let encoder = ReedSolomonEncoder::new(ecc_len);
        let decoder = ReedSolomonDecoder::new(ecc_len);
        
        // Encode
        let encoded = encoder.encode(data);
        assert_eq!(encoded.len(), data.len() + ecc_len);
        
        // Decode without errors
        let result = decoder.decode(&encoded, None);
        assert!(result.success);
        assert_eq!(result.data, data);
        assert_eq!(result.errors_corrected, 0);
    }
    
    #[test]
    fn test_error_correction() {
        let data = b"Test message for Reed-Solomon";
        let ecc_len = 16;
        
        let encoder = ReedSolomonEncoder::new(ecc_len);
        let decoder = ReedSolomonDecoder::new(ecc_len);
        
        // Encode
        let encoded = encoder.encode(data);
        
        // Introduce errors (up to ecc_len/2 can be corrected)
        let mut corrupted = encoded.clone();
        let max_errors = ecc_len / 2;
        for i in 0..max_errors {
            corrupted[i] = corrupted[i].wrapping_add(1);
        }
        
        // Decode and correct
        let result = decoder.decode(&corrupted, None);
        assert!(result.success);
        assert_eq!(result.data, data);
        assert!(result.errors_corrected > 0);
    }
    
    #[test]
    fn test_utility_functions() {
        let data = b"Test default functions";
        
        let encoded = utils::encode_default(data);
        let result = utils::decode_default(&encoded);
        
        assert!(result.success);
        assert_eq!(result.data, data);
    }
}
