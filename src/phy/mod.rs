// Physical layer module for Project 2
// Implements baseband transmission with line coding

pub mod encoder;
pub mod decoder;
pub mod line_coding;
pub mod crc;
pub mod frame;

pub use encoder::PhyEncoder;
pub use decoder::PhyDecoder;
pub use frame::{Frame, FrameType};
