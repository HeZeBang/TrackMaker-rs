use crc32fast::Hasher;
use reed_solomon::Encoder;
use tracing::{debug, error, warn};
use tracing_subscriber::field::debug;

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
    use_reed_solomon: bool,
    ecc_len: usize,
}

impl Framer {
    pub fn new() -> Self {
        Self {
            block_size: 250,
            checksum: Checksum,
            use_reed_solomon: false,
            ecc_len: 8,
        }
    }

    pub fn with_reed_solomon(ecc_len: usize) -> Self {
        // 当使用Reed-Solomon时，需要为纠错码预留空间
        let effective_block_size = if ecc_len > 0 { 250 - ecc_len } else { 250 };
        Self {
            block_size: effective_block_size,
            checksum: Checksum,
            use_reed_solomon: true,
            ecc_len,
        }
    }

    fn pack(&self, block: &[u8]) -> Vec<u8> {
        // 应用Reed-Solomon编码到payload
        let data_with_ecc = if self.use_reed_solomon && self.ecc_len > 0 {
            let mut result = block.to_vec();
            for i in 0..self.ecc_len {
                if !block.is_empty() {
                    let enc = Encoder::new(self.ecc_len);
                    let encoded = enc.encode(block);
                    result.push(encoded.ecc()[i]);
                } else {
                    result.push(0);
                }
            }
            result
        } else {
            block.to_vec()
        };

        // 计算整个data_with_ecc的校验和
        let frame = self
            .checksum
            .encode(&data_with_ecc);
        let mut result = Vec::new();
        result.push(frame.len() as u8);
        result.extend_from_slice(&frame);
        result
    }

    pub fn encode(&self, data: &[u8]) -> impl Iterator<Item = Vec<u8>> + '_ {
        let chunks: Vec<_> = data
            .chunks(self.block_size)
            .collect();
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

pub fn encode_with_reed_solomon(data: &[u8], ecc_len: usize) -> Vec<bool> {
    let framer = Framer::with_reed_solomon(ecc_len);
    let converter = BitPacker::new();

    let mut result = Vec::new();
    for frame in framer.encode(data) {
        println!("Frame encoded with length: {}", frame.len());
        for &byte in &frame {
            result.extend_from_slice(&converter.to_bits[&byte]);
        }
    }
    result
}

// 将位流按低位在前的顺序转为字节流（与 Python BitPacker 匹配）
pub fn bits_to_bytes<I: Iterator<Item = bool>>(
    mut bits: I,
) -> impl Iterator<Item = u8> {
    std::iter::from_fn(move || {
        let mut byte = 0u8;
        for i in 0..8 {
            if let Some(bit) = bits.next() {
                if bit {
                    byte |= 1 << i;
                }
            } else {
                return None;
            }
        }
        Some(byte)
    })
}

// 从位流解码帧：长度前缀(1字节) + [CRC32(4字节)+payload]
// 遇到 frame_len==0 即 EOF，返回 None 终止
pub fn decode_frames_from_bits<I: Iterator<Item = bool>>(
    bits: I,
) -> impl Iterator<Item = Vec<u8>> {
    decode_frames_from_bits_with_params(bits, false, 0)
}

pub fn decode_frames_from_bits_with_reed_solomon<I: Iterator<Item = bool>>(
    bits: I,
    ecc_len: usize,
) -> impl Iterator<Item = Vec<u8>> {
    decode_frames_from_bits_with_params(bits, true, ecc_len)
}

fn decode_frames_from_bits_with_params<I: Iterator<Item = bool>>(
    bits: I,
    use_reed_solomon: bool,
    ecc_len: usize,
) -> impl Iterator<Item = Vec<u8>> {
    let mut bytes = bits_to_bytes(bits);
    std::iter::from_fn(move || {
        // 读取长度
        let len = bytes.next()? as usize;
        if len == 0 {
            return None;
        }
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
        debug!("Frame Actual length: {}", frame.len());
        if frame.len() < 4 {
            warn!(
                "Length Field Mismatch: frame length {}, but expect at least 4",
                frame.len()
            );
            return None;
        }

        let checksum =
            u32::from_be_bytes([frame[0], frame[1], frame[2], frame[3]]);
        let data_with_ecc = &frame[4..];
        let mut hasher = Hasher::new();
        hasher.update(data_with_ecc);
        let checksum_computed = hasher.finalize();
        if checksum_computed != checksum {
            warn!(
                "Hash mismatch: read 0x{:08X}, but expect 0x{:08X}",
                checksum, checksum_computed
            );
            if !use_reed_solomon {
                error!("Checksum mismatch without Reed-Solomon, terminating.");
                return None; // 无纠错时直接终止
            }
        }

        // 处理Reed-Solomon纠错
        let payload =
            if use_reed_solomon && ecc_len > 0 && data_with_ecc.len() >= ecc_len
            {
                // 提取原始payload（去掉Reed-Solomon纠错码）
                let payload_len = data_with_ecc.len() - ecc_len;
                let original_payload = &data_with_ecc[..payload_len];
                original_payload.to_vec()
            } else {
                data_with_ecc.to_vec()
            };

        // EOF After ECC
        if payload.is_empty() {
            warn!("Detected EOF : no payload");
            return None;
        }

        Some(payload)
    })
}
