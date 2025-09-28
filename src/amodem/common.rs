const SCALING: f64 = 32000.0; // out of 2**15

pub fn dumps(samples: &[f64]) -> Vec<u8> {
    let mut result = Vec::new();
    for &sample in samples {
        let scaled = (sample * SCALING) as i16;
        result.extend_from_slice(&scaled.to_le_bytes());
    }
    result
}

pub fn iterate<T>(data: impl Iterator<Item = T>, size: usize) -> impl Iterator<Item = Vec<T>> {
    let mut iter = data;
    std::iter::from_fn(move || {
        let mut chunk = Vec::new();
        for _ in 0..size {
            if let Some(item) = iter.next() {
                chunk.push(item);
            } else {
                break;
            }
        }
        if chunk.is_empty() {
            None
        } else if chunk.len() < size {
            // pad with default values if needed
            None
        } else {
            Some(chunk)
        }
    })
}

pub fn take<T: Copy>(iter: &mut impl Iterator<Item = T>, n: usize) -> Vec<T> {
    iter.take(n).collect()
}
