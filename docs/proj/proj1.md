# Project 1

## PSK 简介

PSK（Phase Shift Keying，相位调制） 是一种数字调制技术，通过改变载波信号的相位来表示不同的数字数据。

- 二进制相位调制（BPSK）：使用两个相位（通常是0度和180度）来表示二进制数据的0和1。这种方法简单且抗噪声能力强，但数据传输速率较低。
- 四进制相位调制（QPSK）：使用四个相位（通常是0度、90度、180度和270度）来表示两位二进制数据（00、01、10、11）。这种方法提高了数据传输速率，但对噪声的敏感性也增加了。
- 多进制相位调制（M-PSK）：使用更多的相位：8-PSK、16-PSK等，可以表示更多的比特数据，但对噪声的敏感性也更高。

### PSK 优劣

- 优点
    - 高抗干扰性：PSK的抗噪声能力较强，尤其是在较低的信噪比条件下（如BPSK）。
    - 频谱效率：相比于幅度调制（AM）和频率调制（FM），PSK可以更有效地利用带宽进行数据传输。
    实现简单：PSK的调制和解调过程相对简单，易于实现。
- 缺点
    - 对信号幅度的要求：PSK对相位的精确度要求较高，在噪声和信号衰减较大的环境中，解调可能会出现误码。
    - 较低的功率效率：与一些其他调制方式（如QAM）相比，PSK的功率效率可能较低。

## PHY Frame

PHY帧（Physical Layer Frame） 是物理层用于传输数据的基本单元。它通常包括了与数据传输、错误检测、信号同步等相关的各种信息。

尽管不同的无线通信标准有各自的PHY帧结构，通常的结构都会包含一些基本的组成部分，如前导符（Preamble）、帧头（Header）、数据部分（Frame Body）和错误检测字段（FCS）。

我们的结构为：

- Preamble
- Len
- CRC
- PHY Payload

## Chirp Signal

Chirp 信号是一种频率调制信号，其频率随时间线性变化，通常用于雷达和通信系统中。它的主要特点是频率从一个初始值线性增加到一个终止值，然后再线性减少回初始值，形成一个“啁啾”效果。

假设一个啁啾信号的频率变化是线性的，它的数学表达式通常为：

$$
s(t) = A \cdot \sin\left(2\pi \left(f_0 t + \frac{K}{2} t^2\right)\right)
$$

其中：
- $s(t)$ 是时间 $t$ 上的信号值。
- $A$ 是信号的振幅。
- $f_0$ 是信号的起始频率。
- $K$ 是频率变化率，定义为 $K = \frac{f_1 - f_0}{T}$，其中 $f_1$ 是终止频率，$T$ 是信号持续的时间。

# 参考的实现

参考 [SamplePHY.m](/SamplePHY.m)，我们分为两个模块：Sender，Receiver

## Sender

### 帧结构

Payload 部分，Sender 首先会生成 100 帧，每帧包含 100 个随机比特，但前八个将会被我们用作帧 ID。

Premble 部分，Sender 生成线性啁啾信号，一共 440 采样，实现 2kHz -> 10kHz, 10kHz -> 2kHz。并且通过累计梯形积分生成平滑的频率调制信号。

### 载波生成

Sender 通过 10kHz 的正弦波作为载波，配以 **ASK 幅移** 调制，每个数据位用44个载波样本表示，通过改变载波幅度来编码数字信息。

```rust
let mut frame_wave = Vec::with_capacity(frame_crc.len() * 44);
for (j, &bit) in frame_crc.iter().enumerate() {
    let start_idx = j * 44;
    let end_idx = (j + 1) * 44;
    let amplitude = if bit == 1 { 1.0 } else { -1.0 }; // 幅度编码
    
    for k in start_idx..end_idx.min(carrier.len()) {
        frame_wave.push(carrier[k] * amplitude); // 载波调制
    }
}
```

编码规则：

- 逻辑1：载波幅度 = +1.0（正相）
- 逻辑0：载波幅度 = -1.0（反相）
- 波特率：44样本/比特 ÷ 48kHz采样率 ≈ 1090 bps

### 帧组装和传输

完整的一帧包含：
- 随机间隔1
- 440 采样前导码
- 数据108位×44采样
- 随机间隔2

```rust
let mut frame_wave_pre = preamble.clone();
frame_wave_pre.extend(frame_wave);

// 添加随机帧间间隔
let inter_frame_space1: usize = rng.random_range(0..100);
let inter_frame_space2: usize = rng.random_range(0..100);

output_track.extend(vec![0.0; inter_frame_space1]);  // 静音间隔
output_track.extend(frame_wave_pre);                 // 完整帧
output_track.extend(vec![0.0; inter_frame_space2]);  // 静音间隔
```

### 校验（未完成）

## Receiver

### 功率检测

用指数移动平均来计算信号功率，通过功率大小实现对噪声的区分

```rust
power = power * (1.0 - 1.0 / 64.0) + current_sample * current_sample / 64.0;
```

### 前导码检测和同步

使用滑动相关器检测同步

```rust
// 维护440样本的滑动窗口
sync_fifo.rotate_left(1);
sync_fifo[439] = current_sample;

// 计算与本地前导码的相关性
let sync_power = sync_fifo
    .iter()
    .zip(preamble.iter())
    .map(|(a, b)| a * b)
    .sum::<f32>()
    / 200.0;

if (sync_power > power * 2.0)           // 相关性 > 平均功率的2倍
    && (sync_power > sync_power_local_max)  // 相关性 > 局部最大值
    && (sync_power > 0.05)               // 相关性 > 绝对阈值
{
    sync_power_local_max = sync_power;
    start_index = i;  // 记录同步点
}
```

### 载波解调

使用相干解调，将接收信号与本地的 10kHz 载波相乘，再通过滑动窗口平滑信号，低通滤波去除高频分量

```rust
let mut decode_remove_carrier = Vec::with_capacity(decode_len);
for j in 0..decode_len {
    let start = j.saturating_sub(5);
    let end = (j + 6).min(decode_len);
    let sum: f32 = (start..end)
        .map(|k| decode_fifo[k] * carrier_slice.get(k).unwrap_or(&0.0))
        .sum();
    decode_remove_carrier.push(sum / (end - start) as f32);
}
```

### 数据比特判定

```rust
let mut decode_power_bit = vec![false; 108];
for j in 0..108 {
    let start_idx = 10 + j * 44;  // 每比特44样本
    let end_idx = (30 + j * 44).min(decode_remove_carrier.len());
    if start_idx < decode_remove_carrier.len() && start_idx < end_idx {
        let sum: f32 = decode_remove_carrier[start_idx..end_idx].iter().sum();
        decode_power_bit[j] = sum > 0.0;  // 正值为1，负值为0
    }
}
```

对每个比特的 20 个采样进行积分，以 0 为界限判定。积分过程能有效抑制随机噪声。


### 帧验证和错误检测

目前只实现了帧 ID 的检测

### 状态转移

- 同步模式在同步完成后进入解码模式
- 解码模式在验证帧完成后回到同步模式