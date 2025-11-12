// Physical layer module for Project 2
// Implements baseband transmission with line coding

pub mod crc;
pub mod decoder;
pub mod encoder;
pub mod frame;
pub mod line_coding;

pub use decoder::PhyDecoder;
pub use encoder::PhyEncoder;
pub use frame::{Frame, FrameType};
