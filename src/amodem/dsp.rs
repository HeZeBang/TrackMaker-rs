use num_complex::Complex64;
use crate::amodem::common;

pub struct Modem {
    encode_map: std::collections::HashMap<Vec<bool>, Complex64>,
    symbols: Vec<Complex64>,
    bits_per_symbol: usize,
}

impl Modem {
    pub fn new(symbols: Vec<Complex64>) -> Self {
        let mut encode_map = std::collections::HashMap::new();
        let bits_per_symbol = (symbols.len() as f64).log2() as usize;
        assert_eq!(2_usize.pow(bits_per_symbol as u32), symbols.len());
        
        for (i, &symbol) in symbols.iter().enumerate() {
            let mut bits = Vec::new();
            for j in 0..bits_per_symbol {
                bits.push((i & (1 << j)) != 0);
            }
            encode_map.insert(bits, symbol);
        }
        
        Self {
            encode_map,
            symbols,
            bits_per_symbol,
        }
    }
    
    pub fn encode(&self, bits: impl Iterator<Item = bool>) -> Vec<Complex64> {
        let bit_vec: Vec<bool> = bits.collect();
        common::iterate(bit_vec.into_iter(), self.bits_per_symbol)
            .map(|bit_chunk| {
                self.encode_map.get(&bit_chunk).copied().unwrap_or(Complex64::new(0.0, 0.0))
            })
            .collect()
    }
    
    pub fn bits_per_symbol(&self) -> usize {
        self.bits_per_symbol
    }
    
    pub fn decode(&self, symbols: Vec<Complex64>) -> Vec<Vec<bool>> {
        symbols.into_iter().map(|received| {
            // Maximum-likelihood decoding using nearest neighbor
            let mut min_error = f64::INFINITY;
            let mut best_bits = vec![false; self.bits_per_symbol];
            
            for (bits, &symbol) in &self.encode_map {
                let error = (received - symbol).norm();
                if error < min_error {
                    min_error = error;
                    best_bits = bits.clone();
                }
            }
            
            best_bits
        }).collect()
    }
}

// Pseudo-random bit sequence generator
pub struct Prbs {
    reg: u32,
    poly: u32,
    mask: u32,
    size: usize,
}

impl Prbs {
    pub fn new(reg: u32, poly: u32, bits: usize) -> Self {
        let mask = (1 << bits) - 1;
        
        let mut size = 0;
        while (poly >> size) > 1 {
            size += 1;
        }
        
        Self { reg, poly, mask, size }
    }
}

impl Iterator for Prbs {
    type Item = u32;
    
    fn next(&mut self) -> Option<Self::Item> {
        let result = self.reg & self.mask;
        self.reg <<= 1;
        if self.reg >> self.size != 0 {
            self.reg ^= self.poly;
        }
        Some(result)
    }
}
