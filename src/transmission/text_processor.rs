/// Text processing utilities for transmission
use std::fs;
use std::path::Path;
use tracing::info;

/// Text processor for reading and validating text data
pub struct TextProcessor;

impl TextProcessor {
    /// Read text from file with fallback
    pub fn read_text_file(file_path: &str) -> String {
        match fs::read_to_string(file_path) {
            Ok(content) => {
                info!("Successfully read text from: {}", file_path);
                content.trim().to_string() // Remove trailing whitespace
            }
            Err(e) => {
                info!("Failed to read file {}: {}, using fallback text", file_path, e);
                "Hello World! This is a fallback message for PSK transmission. 你好世界！".to_string()
            }
        }
    }
    
    /// Save text to file
    pub fn save_text_file(file_path: &Path, content: &str) -> Result<(), Box<dyn std::error::Error>> {
        // Ensure parent directory exists
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent)?;
        }
        
        fs::write(file_path, content)?;
        info!("Text saved to: {}", file_path.display());
        Ok(())
    }
    
    /// Compare two texts and report differences
    pub fn compare_texts(original: &str, received: &str) -> TextComparisonResult {
        let original_trimmed = original.trim();
        let received_trimmed = received.trim();
        
        let is_perfect_match = original_trimmed == received_trimmed;
        
        let mut first_difference = None;
        
        if !is_perfect_match {
            let orig_chars: Vec<char> = original_trimmed.chars().collect();
            let recv_chars: Vec<char> = received_trimmed.chars().collect();
            let min_len = std::cmp::min(orig_chars.len(), recv_chars.len());
            
            for i in 0..min_len {
                if orig_chars[i] != recv_chars[i] {
                    first_difference = Some(TextDifference {
                        position: i,
                        original_char: orig_chars[i],
                        received_char: recv_chars[i],
                    });
                    break;
                }
            }
        }
        
        TextComparisonResult {
            is_perfect_match,
            original_length: original_trimmed.len(),
            received_length: received_trimmed.len(),
            first_difference,
        }
    }
    
    /// Save received text and comparison files
    pub fn save_received_text_with_comparison(
        received_text: &str,
        original_file_path: &str,
        output_dir: &Path,
    ) -> Result<TextComparisonResult, Box<dyn std::error::Error>> {
        // Create output directory if it doesn't exist
        if !output_dir.exists() {
            fs::create_dir_all(output_dir)?;
        }
        
        // Save received text
        let received_file_path = output_dir.join("received_text.txt");
        Self::save_text_file(&received_file_path, received_text)?;
        
        // Load and save original text
        let original_text = Self::read_text_file(original_file_path);
        let original_file_path = output_dir.join("original_text.txt");
        Self::save_text_file(&original_file_path, &original_text)?;
        
        // Compare texts
        let comparison = Self::compare_texts(&original_text, received_text);
        
        // Log comparison results
        if comparison.is_perfect_match {
            info!("✅ TEXT TRANSMISSION PERFECT MATCH!");
        } else {
            info!("⚠️  Text transmission has differences");
            info!("Original length: {} bytes", comparison.original_length);
            info!("Received length: {} bytes", comparison.received_length);
            
            if let Some(diff) = &comparison.first_difference {
                info!("First difference at position {}: '{}' vs '{}'", 
                     diff.position, diff.original_char, diff.received_char);
            }
        }
        
        Ok(comparison)
    }
    
    /// Save raw bytes for debugging
    pub fn save_raw_bytes(bytes: &[u8], file_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent)?;
        }
        
        fs::write(file_path, bytes)?;
        info!("Raw bytes saved to: {}", file_path.display());
        Ok(())
    }
}

/// Result of text comparison
#[derive(Debug)]
pub struct TextComparisonResult {
    pub is_perfect_match: bool,
    pub original_length: usize,
    pub received_length: usize,
    pub first_difference: Option<TextDifference>,
}

/// Details about the first difference found
#[derive(Debug)]
pub struct TextDifference {
    pub position: usize,
    pub original_char: char,
    pub received_char: char,
}
