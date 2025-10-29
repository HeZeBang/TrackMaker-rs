use std::f64::consts::PI;

fn sinc(x: f64) -> f64 {
    if x == 0.0 {
        1.0
    } else {
        (PI * x).sin() / (PI * x)
    }
}

pub struct Interpolator {
    pub width: usize,
    pub resolution: usize,
    pub filt: Vec<Vec<f64>>,
    pub coeff_len: usize,
}

impl Interpolator {
    pub fn new(resolution: usize, width: usize) -> Self {
        let n = resolution * width;
        let u: Vec<f64> = (-(n as i32)..(n as i32))
            .map(|x| x as f64)
            .collect();
        let window: Vec<f64> = u
            .iter()
            .map(|&x| {
                (0.5 * PI * x / (n as f64))
                    .cos()
                    .powi(2)
            })
            .collect();
        let h: Vec<f64> = u
            .iter()
            .zip(window.iter())
            .map(|(&x, &w)| sinc(x / resolution as f64) * w)
            .collect();

        let mut filt = Vec::with_capacity(resolution);
        for index in 0..resolution {
            let mut filt_part: Vec<f64> = h
                .iter()
                .skip(index)
                .step_by(resolution)
                .cloned()
                .collect();
            filt_part.reverse();
            filt.push(filt_part);
        }

        let coeff_len = 2 * width;
        let lengths: Vec<usize> = filt
            .iter()
            .map(|f| f.len())
            .collect();
        assert!(
            lengths
                .iter()
                .all(|&l| l == coeff_len)
        );
        assert_eq!(filt.len(), resolution);

        Self {
            width,
            resolution,
            filt,
            coeff_len,
        }
    }
}

pub fn default_interpolator() -> Interpolator {
    Interpolator::new(1024, 128)
}

pub struct Sampler<I: Iterator<Item = f64>> {
    equalizer: Box<dyn Fn(Vec<f64>) -> Vec<f64>>,
    interp: Interpolator,
    resolution: usize,
    filt: Vec<Vec<f64>>,
    width: usize,
    freq: f64,
    src: std::iter::Chain<std::iter::Take<std::iter::Repeat<f64>>, I>,
    offset: f64,
    buff: Vec<f64>,
    index: usize,
}

impl<I: Iterator<Item = f64>> Sampler<I> {
    pub fn new(signal: I, interp: Option<Interpolator>, freq: f64) -> Self {
        let interp = interp.unwrap_or_else(default_interpolator);
        let resolution = interp.resolution;
        let width = interp.width;
        let filt = interp.filt.clone();
        let equalizer = Box::new(|x: Vec<f64>| x);
        let padding = std::iter::repeat(0.0).take(width);
        let src = padding.chain(signal);
        let offset = (width + 1) as f64;
        let buff = vec![0.0; interp.coeff_len];
        let index = 0;

        Self {
            src,
            equalizer,
            interp,
            resolution,
            filt,
            width,
            freq,
            offset,
            buff,
            index,
        }
    }

    pub fn take(&mut self, size: usize) -> Option<Vec<f64>> {
        if size == 0 {
            return Some(Vec::new());
        }

        let mut frame = vec![0.0; size];
        let mut count = 0;

        for frame_index in 0..size {
            let offset = self.offset;
            let k = offset.floor() as usize;
            let j = ((offset - k as f64) * self.resolution as f64) as usize;
            let coeffs = &self.filt[j];
            let end = k + self.width;

            // Process input until buffer is full with samples
            while self.index < end {
                if let Some(sample) = self.src.next() {
                    // Shift buffer left
                    let buff_len = self.buff.len();
                    self.buff.copy_within(1.., 0);
                    self.buff[buff_len - 1] = sample;
                    self.index += 1;
                } else {
                    // Not enough data
                    return None;
                }
            }

            self.offset += self.freq;

            // Apply interpolation filter
            let sample = coeffs
                .iter()
                .zip(self.buff.iter())
                .map(|(a, b)| a * b)
                .sum();
            frame[frame_index] = sample;
            count = frame_index + 1;
        }

        let result = (self.equalizer)(frame[..count].to_vec());
        Some(result)
    }

    pub fn has_data(&self) -> bool {
        self.index > 0
    }
}
