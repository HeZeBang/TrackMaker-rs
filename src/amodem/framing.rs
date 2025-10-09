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

// 将位流按低位在前的顺序转为字节流（与 Python BitPacker 匹配）
pub fn bits_to_bytes<I: Iterator<Item = bool>>(mut bits: I) -> impl Iterator<Item = u8> {
    std::iter::from_fn(move || {
        let mut byte = 0u8;
        for i in 0..8 {
            if let Some(bit) = bits.next() {
                if bit { byte |= 1 << i; }
            } else {
                return None;
            }
        }
        Some(byte)
    })
}

// 从位流解码帧：长度前缀(1字节) + [CRC32(4字节)+payload]
// 遇到 frame_len==0 即 EOF，返回 None 终止
pub fn decode_frames_from_bits<I: Iterator<Item = bool>>(bits: I) -> impl Iterator<Item = Vec<u8>> {
    let mut bytes = bits_to_bytes(bits);
    std::iter::from_fn(move || {
        // 读取长度
        let len = bytes.next()? as usize;
        if len == 0 { return None; }
        // 读取帧数据 len 字节
        let mut frame = Vec::with_capacity(len);
        for _ in 0..len {
            if let Some(b) = bytes.next() {
                frame.push(b);
            } else {
                return None; // 不完整，终止
            }
        }
        // 校验并输出 payload
        if frame.len() < 4 { return None; }
        let checksum = u32::from_be_bytes([frame[0], frame[1], frame[2], frame[3]]);
        let payload = &frame[4..];
        let mut hasher = Hasher::new();
        hasher.update(payload);
        if hasher.finalize() != checksum {
            return None; // 校验失败，终止本次
        }
        Some(payload.to_vec())
    })
}

pub fn decode(data: &[u8]) -> Result<Vec<u8>, String> {
    let mut result = Vec::new();
    let mut offset = 0;
    
    while offset < data.len() {
        // Read frame length
        if offset >= data.len() {
            break;
        }
        let frame_len = data[offset] as usize;
        offset += 1;
        
        if frame_len == 0 {
            // EOF frame
            break;
        }
        
        // Read frame data
        if offset + frame_len > data.len() {
            return Err("Incomplete frame".to_string());
        }
        
        let frame_data = &data[offset..offset + frame_len];
        offset += frame_len;
        
        // Verify checksum
        if frame_data.len() < 4 {
            return Err("Frame too short for checksum".to_string());
        }
        
        let expected_checksum = u32::from_be_bytes([
            frame_data[0], frame_data[1], frame_data[2], frame_data[3]
        ]);
        let payload = &frame_data[4..];
        
        let mut hasher = Hasher::new();
        hasher.update(payload);
        let actual_checksum = hasher.finalize();
        
        if expected_checksum != actual_checksum {
            return Err("Checksum mismatch".to_string());
        }
        
        result.extend_from_slice(payload);
    }
    
    Ok(result)
}
