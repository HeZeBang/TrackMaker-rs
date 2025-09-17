use symphonia::core::codecs::{CODEC_TYPE_NULL, DecoderOptions};
use symphonia::core::formats::FormatOptions;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::probe::Hint;

fn decoderDemo(file_path: &str) {
    // Open the media source.
    let src = std::fs::File::open(file_path).unwrap();
}