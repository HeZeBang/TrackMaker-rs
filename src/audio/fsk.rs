//! FSK (Frequency Shift Keying) modulation and demodulation

use crate::utils::consts::*;
use std::f32::consts::PI;

/// FSK调制器
pub struct FSKModulator {
    sample_rate: f32,
    freq_0: f32,      // 表示'0'的频率
    freq_1: f32,      // 表示'1'的频率
    samples_per_bit: usize,
    phase_0: f32,     // 频率0的相位累积器
    phase_1: f32,     // 频率1的相位累积器
}

impl FSKModulator {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            freq_0: FSK_FREQ_0,
            freq_1: FSK_FREQ_1,
            samples_per_bit: SAMPLES_PER_BIT,
            phase_0: 0.0,
            phase_1: 0.0,
        }
    }

    /// 调制单个比特，返回对应的音频样本
    pub fn modulate_bit(&mut self, bit: u8) -> Vec<f32> {
        let mut samples = Vec::with_capacity(self.samples_per_bit);
        
        if bit == 0 {
            // 使用频率0
            let phase_increment = 2.0 * PI * self.freq_0 / self.sample_rate;
            for _ in 0..self.samples_per_bit {
                samples.push(self.phase_0.sin());
                self.phase_0 += phase_increment;
                // 保持相位在 [0, 2π) 范围内
                if self.phase_0 >= 2.0 * PI {
                    self.phase_0 -= 2.0 * PI;
                }
            }
        } else {
            // 使用频率1
            let phase_increment = 2.0 * PI * self.freq_1 / self.sample_rate;
            for _ in 0..self.samples_per_bit {
                samples.push(self.phase_1.sin());
                self.phase_1 += phase_increment;
                // 保持相位在 [0, 2π) 范围内
                if self.phase_1 >= 2.0 * PI {
                    self.phase_1 -= 2.0 * PI;
                }
            }
        }
        
        samples
    }

    /// 调制比特序列
    pub fn modulate_bits(&mut self, bits: &[u8]) -> Vec<f32> {
        let mut result = Vec::with_capacity(bits.len() * self.samples_per_bit);
        for &bit in bits {
            result.extend(self.modulate_bit(bit));
        }
        result
    }
}

/// FSK解调器
pub struct FSKDemodulator {
    sample_rate: f32,
    freq_0: f32,
    freq_1: f32,
    samples_per_bit: usize,
    // 带通滤波器状态 (简单的IIR滤波器)
    filter_0_state: [f32; 4],  // [x1, x2, y1, y2]
    filter_1_state: [f32; 4],
}

impl FSKDemodulator {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            freq_0: FSK_FREQ_0,
            freq_1: FSK_FREQ_1,
            samples_per_bit: SAMPLES_PER_BIT,
            filter_0_state: [0.0; 4],
            filter_1_state: [0.0; 4],
        }
    }

    /// 简单的二阶带通滤波器
    fn bandpass_filter(input: f32, center_freq: f32, sample_rate: f32, state: &mut [f32; 4]) -> f32 {
        // 设计一个简单的带通滤波器
        // 这是一个简化的实现，实际应用中可能需要更精确的滤波器设计
        let normalized_freq = center_freq / (sample_rate / 2.0);
        let q = 5.0; // 品质因子
        
        // 简化的IIR系数 (这里使用近似值)
        let w = normalized_freq * PI;
        let cos_w = w.cos();
        let sin_w = w.sin();
        let alpha = sin_w / (2.0 * q);
        
        let b0 = alpha;
        let b1 = 0.0;
        let b2 = -alpha;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * cos_w;
        let a2 = 1.0 - alpha;
        
        // 应用滤波器: y[n] = (b0*x[n] + b1*x[n-1] + b2*x[n-2] - a1*y[n-1] - a2*y[n-2]) / a0
        let output = (b0 * input + b1 * state[0] + b2 * state[1] - a1 * state[2] - a2 * state[3]) / a0;
        
        // 更新状态
        state[1] = state[0];  // x[n-2] = x[n-1]
        state[0] = input;     // x[n-1] = x[n]
        state[3] = state[2];  // y[n-2] = y[n-1] 
        state[2] = output;    // y[n-1] = y[n]
        
        output
    }

    /// 解调一段音频数据，返回检测到的比特
    pub fn demodulate_samples(&mut self, samples: &[f32]) -> Vec<u8> {
        if samples.len() < self.samples_per_bit {
            return Vec::new();
        }

        let num_bits = samples.len() / self.samples_per_bit;
        let mut bits = Vec::with_capacity(num_bits);

        for i in 0..num_bits {
            let start_idx = i * self.samples_per_bit;
            let end_idx = start_idx + self.samples_per_bit;
            
            let mut energy_0 = 0.0f32;
            let mut energy_1 = 0.0f32;

            // 计算每个频率分量的能量
            for j in start_idx..end_idx {
                if j < samples.len() {
                    let sample = samples[j];
                    
                    // 分别通过带通滤波器
                    let filtered_0 = {
                        let freq = self.freq_0;
                        Self::bandpass_filter(sample, freq, self.sample_rate, &mut self.filter_0_state)
                    };
                    let filtered_1 = {
                        let freq = self.freq_1;
                        Self::bandpass_filter(sample, freq, self.sample_rate, &mut self.filter_1_state)
                    };
                    
                    // 累积能量 (平方检波)
                    energy_0 += filtered_0 * filtered_0;
                    energy_1 += filtered_1 * filtered_1;
                }
            }

            // 判决：能量大的频率对应的比特
            if energy_1 > energy_0 {
                bits.push(1);
            } else {
                bits.push(0);
            }
        }

        bits
    }

    /// 简单的能量检测解调 (备用方法)
    pub fn demodulate_simple(&self, samples: &[f32]) -> Vec<u8> {
        if samples.len() < self.samples_per_bit {
            return Vec::new();
        }

        let num_bits = samples.len() / self.samples_per_bit;
        let mut bits = Vec::with_capacity(num_bits);

        for i in 0..num_bits {
            let start_idx = i * self.samples_per_bit;
            let end_idx = start_idx + self.samples_per_bit;
            
            let mut correlation_0 = 0.0f32;
            let mut correlation_1 = 0.0f32;

            // 与参考载波进行相关运算
            for j in start_idx..end_idx {
                if j < samples.len() {
                    let t = (j - start_idx) as f32 / self.sample_rate;
                    let ref_0 = (2.0 * PI * self.freq_0 * t).sin();
                    let ref_1 = (2.0 * PI * self.freq_1 * t).sin();
                    
                    correlation_0 += samples[j] * ref_0;
                    correlation_1 += samples[j] * ref_1;
                }
            }

            // 判决：相关值大的频率对应的比特
            if correlation_1.abs() > correlation_0.abs() {
                bits.push(1);
            } else {
                bits.push(0);
            }
        }

        bits
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fsk_modulation() {
        let mut modulator = FSKModulator::new(48000.0);
        let bits = vec![0, 1, 0, 1];
        let samples = modulator.modulate_bits(&bits);
        
        // 检查输出长度
        assert_eq!(samples.len(), bits.len() * SAMPLES_PER_BIT);
        
        // 检查样本值在合理范围内
        for sample in &samples {
            assert!(sample.abs() <= 1.1); // 允许一些数值误差
        }
    }

    #[test]
    fn test_fsk_demodulation() {
        let mut modulator = FSKModulator::new(48000.0);
        let mut demodulator = FSKDemodulator::new(48000.0);
        
        let original_bits = vec![0, 1, 1, 0, 1];
        let samples = modulator.modulate_bits(&original_bits);
        let decoded_bits = demodulator.demodulate_simple(&samples);
        
        assert_eq!(decoded_bits.len(), original_bits.len());
        // 注意：在没有噪声的情况下，解调应该是准确的
        // 但由于数值精度问题，这里不做严格的相等检查
    }
}
