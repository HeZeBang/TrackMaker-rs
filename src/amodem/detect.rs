use crate::amodem::{common, config::Configuration, equalizer};
use num_complex::Complex64;
use std::collections::VecDeque;
use tracing::debug;

pub struct Detector {
    freq: f64,
    omega: f64,
    nsym: usize,
    tsym: f64,
    maxlen: usize,
    max_offset: usize,

    // Detection thresholds
    coherence_threshold: f64,
    carrier_duration: usize,
    carrier_threshold: usize,
    search_window: usize,
    start_pattern_length: usize,
}

impl Detector {
    pub fn new(config: &Configuration) -> Self {
        let freq = config.fc;
        let omega = 2.0 * std::f64::consts::PI * freq / config.fs;
        let nsym = config.nsym;
        let tsym = config.tsym;
        let maxlen = config.baud; // 1 second of symbols
        let max_offset = (config.timeout * config.fs) as usize;

        // Python constants
        let coherence_threshold = 0.9;
        let carrier_duration = equalizer::get_prefix()
            .iter()
            .sum::<f64>() as usize;
        let carrier_threshold = (0.9 * carrier_duration as f64) as usize;
        let search_window = (0.1 * carrier_duration as f64) as usize;
        let start_pattern_length = search_window / 4;

        Self {
            freq,
            omega,
            nsym,
            tsym,
            maxlen,
            max_offset,
            coherence_threshold,
            carrier_duration,
            carrier_threshold,
            search_window,
            start_pattern_length,
        }
    }

    pub fn run(
        &self,
        mut samples: impl Iterator<Item = f64>,
    ) -> Result<(Vec<f64>, f64, f64), String> {
        // 1) 等待载波：按符号切块、计算相干度、累计达到阈值
        let (offset_samples, mut bufs) = self.wait_for_carrier(&mut samples)?;

        // 2) 计算开始时间日志（近似显示）
        let start_time_ms =
            (offset_samples as f64 / self.nsym as f64) * self.tsym * 1e3;
        eprintln!(
            "Carrier detected at ~{:.1} ms @ {:.1} kHz",
            start_time_ms,
            self.freq / 1e3
        );

        // 3) 组装用于精确定位的缓冲：取尾部窗口 + 额外追踪
        let search_window = self.search_window;
        let carrier_duration = self.carrier_duration;
        let carrier_threshold = self.carrier_threshold;

        // 仅保留最近 (carrier_threshold + search_window) 个符号块
        let keep = carrier_threshold + search_window;
        if bufs.len() > keep {
            bufs.drain(0..(bufs.len() - keep));
        }

        // 需要从后续 samples 再补充 n 个符号块样本
        let n_trailing = search_window + carrier_duration - carrier_threshold;
        let mut trailing: Vec<f64> = Vec::with_capacity(n_trailing * self.nsym);
        for _ in 0..(n_trailing * self.nsym) {
            if let Some(v) = samples.next() {
                trailing.push(v);
            } else {
                break;
            }
        }

        // 拼接 buf 以做相关
        let mut buf: Vec<f64> = bufs
            .into_iter()
            .flatten()
            .collect();
        buf.extend(trailing.iter());

        // 4) 相关搜索精确定位起点
        let offset_in_buf = self.find_start(&buf);
        let start_time_ms = ((offset_samples as f64
            - (carrier_threshold - 1) as f64 * self.nsym as f64)
            / self.nsym as f64
            + (offset_in_buf as f64 / self.nsym as f64
                - self.search_window as f64))
            * self.tsym
            * 1e3;
        eprintln!("Carrier starts at {:.3} ms", start_time_ms);

        // 5) 估计幅度与频偏（在前导长度范围内）
        let prefix_len = carrier_duration * self.nsym;
        let slice_end = (offset_in_buf + prefix_len).min(buf.len());
        let amplitude;
        let freq_err;
        if offset_in_buf < slice_end {
            let est_on = &buf[offset_in_buf..slice_end];
            let (a, f) = self.estimate(est_on);
            amplitude = a;
            freq_err = f;
        } else {
            amplitude = 1.0;
            freq_err = 0.0;
        }

        debug!("Estimated amplitude: {:.3}", amplitude);
        debug!("Estimated frequency error: {:.3} ppm", freq_err * 1e6);

        // 6) 返回：从精确起点后的 buf + 尚未消耗的 samples 组成的完整后续信号
        let mut final_signal = buf[offset_in_buf..].to_vec();
        final_signal.extend(samples);

        Ok((final_signal, amplitude, freq_err))
    }

    fn wait_for_carrier(
        &self,
        samples: impl Iterator<Item = f64>,
    ) -> Result<(usize, Vec<Vec<f64>>), String> {
        let mut counter = 0;
        let mut bufs = VecDeque::with_capacity(self.maxlen);

        for (offset, buf) in common::iterate(samples, self.nsym).enumerate() {
            if offset * self.nsym > self.max_offset {
                return Err("Timeout waiting for carrier".to_string());
            }

            if bufs.len() >= self.maxlen {
                bufs.pop_front();
            }
            bufs.push_back(buf);

            let coeff = self.coherence(&bufs.back().unwrap());
            if coeff.norm() > self.coherence_threshold {
                counter += 1;
            } else {
                counter = 0;
            }

            if counter == self.carrier_threshold {
                return Ok((offset * self.nsym, bufs.into_iter().collect()));
            }
        }

        Err("No carrier detected".to_string())
    }

    fn coherence(&self, buf: &[f64]) -> Complex64 {
        let n = buf.len();
        let hc: Vec<Complex64> = (0..n)
            .map(|i| {
                let phase = -self.omega * i as f64;
                Complex64::new(phase.cos(), phase.sin())
                    / (0.5 * n as f64).sqrt()
            })
            .collect();

        let norm_x = buf
            .iter()
            .map(|&x| x * x)
            .sum::<f64>()
            .sqrt();
        if norm_x == 0.0 {
            return Complex64::new(0.0, 0.0);
        }

        let dot_product: Complex64 = hc
            .iter()
            .zip(buf.iter())
            .map(|(&h, &x)| h * x)
            .sum();

        dot_product / norm_x
    }

    // 归一化互相关定位起点
    fn find_start(&self, buf: &[f64]) -> usize {
        // 1. 构造复数模板 (I/Q)
        let mut carrier: Vec<Complex64> =
            Vec::with_capacity(self.nsym * self.start_pattern_length);
        for _ in 0..self.start_pattern_length {
            for i in 0..self.nsym {
                let phase = self.omega * i as f64;
                carrier.push(Complex64::new(phase.cos(), phase.sin()));
            }
        }

        let zeroes_len = carrier.len();
        let mut tmpl: Vec<Complex64> =
            vec![Complex64::new(0.0, 0.0); zeroes_len];
        tmpl.extend(carrier);

        // 2. 归一化模板 (与 Python 对齐)
        let tmpl_norm = tmpl
            .iter()
            .map(|c| c.norm_sqr())
            .sum::<f64>()
            .sqrt();
        if tmpl_norm == 0.0 {
            return 0;
        }
        let scale = 2.0f64.sqrt() / tmpl_norm;
        let tmpl: Vec<Complex64> = tmpl
            .into_iter()
            .map(|c| c * scale)
            .collect();

        if buf.len() < tmpl.len() {
            return 0;
        }

        // 3. 计算相关性
        let mut best_idx = 0;
        let mut max_coeff = 0.0;

        // 计算滑动窗口能量
        let mut window_energies = Vec::with_capacity(buf.len() - tmpl.len() + 1);
        let mut current_energy_sq = buf[0..tmpl.len()]
            .iter()
            .map(|&x| x * x)
            .sum::<f64>();
        window_energies.push(current_energy_sq.sqrt());

        for i in 1..=(buf.len() - tmpl.len()) {
            let old_val = buf[i - 1];
            let new_val = buf[i + tmpl.len() - 1];
            current_energy_sq =
                current_energy_sq - old_val * old_val + new_val * new_val;
            window_energies.push(current_energy_sq.sqrt());
        }

        for i in 0..=(buf.len() - tmpl.len()) {
            let window = &buf[i..i + tmpl.len()];

            // 复数相关
            let corr: Complex64 = window
                .iter()
                .zip(tmpl.iter())
                .map(|(&x, &t)| t.conj() * x)
                .sum();

            let norm_b = window_energies[i];
            let coeff = if norm_b > 0.0 {
                corr.norm() / norm_b
            } else {
                0.0
            };

            if coeff > max_coeff {
                max_coeff = coeff;
                best_idx = i;
            }
        }

        debug!("Carrier coherence: {:.3}%", max_coeff * 100.0);
        best_idx + zeroes_len
    }

    // 估计幅度与频偏
    fn estimate(&self, buf: &[f64]) -> (f64, f64) {
        if buf.len() < self.nsym * 3 {
            return (1.0, 0.0);
        }
        let mut symbols: Vec<Complex64> = Vec::new();
        for frame in buf.chunks(self.nsym) {
            if frame.len() < self.nsym {
                break;
            }
            let mut sum = Complex64::new(0.0, 0.0);
            let scale = 0.5 * frame.len() as f64;
            for (i, &x) in frame.iter().enumerate() {
                let phase = -self.omega * i as f64;
                let w = Complex64::new(phase.cos(), phase.sin()) / scale;
                sum += w * x;
            }
            symbols.push(sum);
        }
        let skip = 5usize;
        let symbols = if symbols.len() > 2 * skip {
            &symbols[skip..symbols.len() - skip]
        } else {
            &symbols[..]
        };
        if symbols.is_empty() {
            return (1.0, 0.0);
        }
        let amplitude = symbols
            .iter()
            .map(|c| c.norm())
            .sum::<f64>()
            / (symbols.len() as f64);

        let mut phases: Vec<f64> = symbols
            .iter()
            .map(|c| c.arg() / (2.0 * std::f64::consts::PI))
            .collect();
        self.unwrap_phases(&mut phases);
        let n = phases.len();
        if n < 2 {
            return (amplitude, 0.0);
        }
        let x_mean = (n as f64 - 1.0) / 2.0;
        let y_mean = phases.iter().sum::<f64>() / n as f64;
        let mut num = 0.0;
        let mut den = 0.0;
        for (i, &y) in phases.iter().enumerate() {
            let xi = i as f64;
            num += (xi - x_mean) * (y - y_mean);
            den += (xi - x_mean) * (xi - x_mean);
        }
        let a = if den != 0.0 { num / den } else { 0.0 };
        let b = y_mean - a * x_mean;
        let freq_err = a / (self.tsym * self.freq);
        debug!("Linear regression: y = {:.6}x + {:.6}", a, b);
        debug!("Frequency error: {:.3} ppm", freq_err * 1e6);
        (amplitude, freq_err)
    }

    fn unwrap_phases(&self, phases: &mut [f64]) {
        if phases.len() < 2 {
            return;
        }

        for i in 1..phases.len() {
            let mut diff = phases[i] - phases[i - 1];

            // 处理相位跳跃
            while diff > 0.5 {
                phases[i] -= 1.0; // 注意这里已经除以了2π
                diff -= 1.0;
            }
            while diff < -0.5 {
                phases[i] += 1.0;
                diff += 1.0;
            }
        }
    }
}
