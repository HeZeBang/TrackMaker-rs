pub const ECC_LEN: usize = 0;
pub const PREAMBLE_LENGTH: usize = 480;
pub const PREAMBLE_FRE_MIN: f32 = 10.0;
pub const PREAMBLE_FRE_MAX: f32 = 50.0;
pub const FRAME_LEN: usize = 125;
pub const NUM_SAMPLES: usize = 4;
pub const DEVICE_ID: f32 = 1.0;
pub const SERVER_ID: f32 = 0.0;
pub const DATA_TYPE: f32 = -0.0;
pub const ACK_TYPE: f32 = 1.0;
pub const HEAD_LEN: usize = 4;

pub const FRAME_PAYLOAD_BYTES: usize = FRAME_LEN;

/// Total bytes in a modulated frame (header + payload + ecc)
pub const FRAME_TOTAL_BYTES: usize = HEAD_LEN + FRAME_LEN + ECC_LEN;

pub const BITS_PER_SYMBOL: usize = 10;

pub const SAMPLES_PER_BIT: usize = NUM_SAMPLES;

pub const SAMPLES_PER_BYTE: usize = BITS_PER_SYMBOL * SAMPLES_PER_BIT;

pub const HEADER_SAMPLES: usize = HEAD_LEN * SAMPLES_PER_BYTE;

pub const FRAME_DATA_SAMPLES: usize = FRAME_LEN * SAMPLES_PER_BYTE;

pub const FRAME_TOTAL_SAMPLES: usize = FRAME_TOTAL_BYTES * SAMPLES_PER_BYTE;
