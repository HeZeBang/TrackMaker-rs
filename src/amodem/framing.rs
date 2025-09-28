use crc32fast::Hasher;

pub struct Checksum;

impl Checksum {
    pub fn encode(&self, payload: &[u8]) -> Vec<u8> {
        let mut hasher = Hasher::new();
        hasher.update(payload);
        let checksum = hasher.finalize();
        
        let mut result = Vec::new();
        result.extend_from_slice(&checksum.to_be_bytes());
        result.extend_from_slice(payload);
        result
    }
}

pub struct Framer {
    block_size: usize,
    checksum: Checksum,
}

impl Framer {
    pub fn new() -> Self {
        Self {
            block_size: 250,
            checksum: Checksum,
        }
    }
    
    fn pack(&self, block: &[u8]) -> Vec<u8> {
        let frame = self.checksum.encode(block);
        let mut result = Vec::new();
        result.push(frame.len() as u8);
        result.extend_from_slice(&frame);
        result
    }
    
    pub fn encode(&self, data: &[u8]) -> impl Iterator<Item = Vec<u8>> + '_ {
        let chunks: Vec<_> = data.chunks(self.block_size).collect();
        let mut frames = Vec::new();
        
        for chunk in chunks {
            frames.push(self.pack(chunk));
        }
        
        // Add EOF frame
        frames.push(self.pack(&[]));
        
        frames.into_iter()
    }
}

pub struct BitPacker {
    to_bits: std::collections::HashMap<u8, Vec<bool>>,
}

impl BitPacker {
    pub fn new() -> Self {
        let mut to_bits = std::collections::HashMap::new();
        
        for i in 0..=255u8 {
            let mut bits = Vec::new();
            for k in 0..8 {
                bits.push((i & (1 << k)) != 0);
            }
            to_bits.insert(i, bits);
        }
        
        Self { to_bits }
    }
}

pub fn encode(data: &[u8]) -> Vec<bool> {
    let framer = Framer::new();
    let converter = BitPacker::new();
    
    let mut result = Vec::new();
    for frame in framer.encode(data) {
        for byte in frame {
            result.extend_from_slice(&converter.to_bits[&byte]);
        }
    }
    result
}
