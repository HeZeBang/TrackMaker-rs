# 采样率设置指南

本指南介绍如何在 trackmaker-rs 项目中设置和使用自定义采样率。

## 什么是采样率？

采样率（Sample Rate）是指每秒对音频信号进行采样的次数，单位为赫兹（Hz）。常见的采样率包括：

- **44.1 kHz** - CD 音质标准
- **48 kHz** - 专业音频和视频标准
- **96 kHz** - 高分辨率音频
- **192 kHz** - 超高分辨率音频

## 如何设置采样率

### 1. 使用命令行参数

现在您可以通过命令行参数来指定采样率：

```bash
# 使用 48 kHz 采样率
cargo run --example tune -- --sample-rate 48000

# 使用 96 kHz 采样率
cargo run --example tune -- --sample-rate 96000

# 使用默认采样率（不指定参数）
cargo run --example tune
```

### 2. 使用示例程序

我们提供了一个专门的示例程序来演示采样率设置：

```bash
# 播放 440 Hz 音调，使用 48 kHz 采样率
cargo run --example sample_rate_demo -- --sample-rate 48000 --frequency 440

# 播放 1000 Hz 音调，使用 96 kHz 采样率
cargo run --example sample_rate_demo -- --sample-rate 96000 --frequency 1000
```

### 3. 在代码中设置采样率

如果您想在代码中直接设置采样率，可以这样做：

```rust
use cpal::{SampleRate, StreamConfig};

// 创建自定义配置
let config = StreamConfig {
    channels: 2,  // 立体声
    sample_rate: SampleRate(48000),  // 48 kHz
    buffer_size: cpal::BufferSize::Default,
};
```

## 支持的采样率

不同的音频设备支持不同的采样率范围。程序会自动检查设备是否支持您指定的采样率：

- 如果支持，将使用您指定的采样率
- 如果不支持，将回退到设备的默认采样率并显示警告

## 常见问题

### Q: 为什么我的采样率设置没有生效？

A: 可能的原因：
1. 您的音频设备不支持指定的采样率
2. 系统音频设置覆盖了应用程序设置
3. 其他应用程序正在使用音频设备

### Q: 如何查看设备支持的采样率？

A: 运行程序时会显示设备的默认配置和支持的配置范围。

### Q: 采样率越高越好吗？

A: 不一定。更高的采样率需要更多的计算资源和存储空间，但可能提供更好的音质。对于大多数应用，44.1 kHz 或 48 kHz 已经足够。

## 示例用法

```bash
# 基本用法
cargo run --example tune -- --sample-rate 44100

# 使用 JACK（如果可用）
cargo run --example tune --features jack -- --jack --sample-rate 48000

# 指定音频设备
cargo run --example tune -- --device "USB Audio Device" --sample-rate 96000
```

## 技术细节

程序使用 CPAL（Cross-Platform Audio Library）来处理音频配置。采样率设置通过以下步骤实现：

1. 获取设备的支持配置列表
2. 检查指定采样率是否在支持范围内
3. 创建相应的音频配置
4. 如果采样率不支持，回退到默认配置
