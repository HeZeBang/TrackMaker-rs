# 自动化测试脚本使用指南

## 概述

`test.py` 是一个自动化测试脚本，用于测试不同参数配置下的音频传输性能。脚本会：

1. 定义多个参数配置
2. 对每个配置修改 `consts.rs`
3. 编译项目
4. 启动发送端和接收端进程
5. 测量运行时间
6. 绘制性能分析图表

## 前提条件

### 安装依赖

```bash
# 安装 Python 依赖
pip install -r tools/requirements.txt
```

### 系统要求

- Python 3.7+
- Rust 和 Cargo（用于编译项目）
- 足够的磁盘空间（用于构建多个版本）

## 使用方法

### 基本运行

```bash
cd /home/zambar/repos/trackmaker-rs
python3 tools/test.py
```

### 脚本流程

脚本会自动执行以下步骤：

1. **参数定义**：定义多个参数配置（见下文）
2. **循环测试**：对每个配置运行 4 次重复测试
3. **进程管理**：
   - 启动两个 TX（发送）进程：`tx -l 1 -r 2` 和 `tx -l 2 -r 1`
   - 启动两个 RX（接收）进程：`rx -l 2 -r 1 -d 40` 和 `rx -l 1 -r 2 -d 40`
   - 当所有 TX 和所有 RX 都完成时，终止剩余进程
4. **结果收集**：记录各进程的运行时间
5. **结果保存**：将结果保存到 JSON 文件和 PNG 图表

## 参数配置

脚本预定义了以下参数配置（在 `ExperimentConfig.get_parameter_combinations()` 中定义）：

### 1. baseline（基准配置）
- SAMPLES_PER_LEVEL: 3
- PREAMBLE_PATTERN_BYTES: 4
- DIFS_DURATION_MS: 20
- CW_MIN: 10
- CW_MAX: 200
- SLOT_TIME_MS: 5

### 2. fast_channel_sense（快速信道感知）
较小的参数值，更快的运行速度
- SAMPLES_PER_LEVEL: 2
- PREAMBLE_PATTERN_BYTES: 4
- DIFS_DURATION_MS: 10
- CW_MIN: 5
- CW_MAX: 100
- SLOT_TIME_MS: 3

### 3. robust_encoding（鲁棒编码）
较大的参数值，更强的容错能力
- SAMPLES_PER_LEVEL: 5
- PREAMBLE_PATTERN_BYTES: 8
- DIFS_DURATION_MS: 20
- CW_MIN: 10
- CW_MAX: 200
- SLOT_TIME_MS: 5

### 4. aggressive_backoff（激进退避）
较小的 CW 和 SLOT_TIME，更快的重试
- SAMPLES_PER_LEVEL: 3
- PREAMBLE_PATTERN_BYTES: 4
- DIFS_DURATION_MS: 20
- CW_MIN: 2
- CW_MAX: 32
- SLOT_TIME_MS: 2

### 5. conservative_backoff（保守退避）
较大的 CW 和 SLOT_TIME，更保守的重试策略
- SAMPLES_PER_LEVEL: 3
- PREAMBLE_PATTERN_BYTES: 4
- DIFS_DURATION_MS: 30
- CW_MIN: 20
- CW_MAX: 400
- SLOT_TIME_MS: 8

### 自定义参数

编辑 `test.py` 中的 `ExperimentConfig.get_parameter_combinations()` 方法来添加或修改参数组合：

```python
def get_parameter_combinations(self) -> List[Dict[str, Any]]:
    return [
        {
            "name": "my_config",
            "SAMPLES_PER_LEVEL": 4,
            "PREAMBLE_PATTERN_BYTES": 6,
            "DIFS_DURATION_MS": 25,
            "CW_MIN": 8,
            "CW_MAX": 150,
            "SLOT_TIME_MS": 4,
        },
        # 添加更多配置...
    ]
```

## 输出文件

脚本会在 `tmp/experiment_logs/` 目录下生成：

### 1. JSON 结果文件
- 文件名：`results_YYYYMMDD_HHMMSS.json`
- 内容：完整的测试结果数据，包括每个重复的详细时间

### 2. PNG 图表文件
- 文件名：`results_YYYYMMDD_HHMMSS.png`
- 包含 4 个子图：
  1. **最大运行时间对比**：展示各配置的平均运行时间和标准差
  2. **详细时间分布**：展示每个配置的所有运行时间
  3. **SAMPLES_PER_LEVEL 影响**：分析 SAMPLES_PER_LEVEL 对性能的影响
  4. **CW_MIN 影响**：分析 CW_MIN（退避窗口）对性能的影响

## 过程日志

每个测试的详细输出保存在 `tmp/` 目录下：
- `tx1.log` - 第一个发送端的输出
- `tx2.log` - 第二个发送端的输出
- `rx1.log` - 第一个接收端的输出
- `rx2.log` - 第二个接收端的输出

## 参数说明

### 物理层参数
- **SAMPLES_PER_LEVEL**：每个编码级别的采样数，越小编码速度越快，但容错能力越弱
- **PREAMBLE_PATTERN_BYTES**：前导码的字节数，用于同步，越多越容易检测但增加开销

### 信道访问参数
- **DIFS_DURATION_MS**：分布式帧间间隔（毫秒），信道感知等待时间
- **CW_MIN**：最小竞争窗口大小，影响首次退避的随机范围
- **CW_MAX**：最大竞争窗口大小，影响重试次数过多后的退避范围
- **SLOT_TIME_MS**：退避槽长度（毫秒），影响退避精度

## 示例输出

```
================================================================================
🚀 Starting Automated Testing
================================================================================

📊 Test Config: baseline (Repeat 1/4)
  Parameters:
    - SAMPLES_PER_LEVEL: 3
    - PREAMBLE_PATTERN_BYTES: 4
    - DIFS_DURATION_MS: 20
    - CW_MIN: 10
    - CW_MAX: 200
    - SLOT_TIME_MS: 5
  Updating consts.rs...
  Building...
  Starting processes...
  ✓ Started tx1 (PID: 12345)
  ✓ Started tx2 (PID: 12346)
  ✓ Started rx2 (PID: 12347)
  ✓ Started rx1 (PID: 12348)
  Waiting for processes to complete...
    ✓ tx1 completed (23.45s)
    ✓ tx2 completed (25.12s)
    ✓ rx1 completed (40.00s)
    ✓ rx2 completed (39.87s)
  All processes completed!
  Results:
    - TX1 time: 23.45s
    - TX2 time: 25.12s
    - RX1 time: 40.00s
    - RX2 time: 39.87s
  ...

📝 Results saved to: tmp/experiment_logs/results_20231116_120000.json
📊 Plot saved to: tmp/experiment_logs/results_20231116_120000.png

================================================================================
✅ All experiments completed!
================================================================================
```

## 故障排除

### 编译失败
- 检查 `consts.rs` 是否被正确修改
- 确保 Rust 编译器已安装并且是最新版本
- 清理 build 缓存：`cargo clean`

### 进程超时
- 增加 `total_timeout` 值（在脚本中）
- 检查系统资源是否充足
- 检查输入文件是否存在

### 图表生成失败
- 确保 matplotlib 和 numpy 已安装
- 检查磁盘空间是否充足
- 尝试更新依赖：`pip install --upgrade -r tools/requirements.txt`

## 性能优化建议

1. **加快编译**：使用 `cargo build --release` 可以获得最佳性能
   - 脚本已自动使用 `--release` 标志
   
2. **并行测试**：如果有多个 CPU，可以修改脚本以支持并行执行不同的配置
   
3. **选择性测试**：注释掉不需要的参数配置以加快测试速度

4. **预热编译**：首次运行时可能较慢，后续运行会更快

## 扩展和修改

### 添加新的参数配置

在 `ExperimentConfig.get_parameter_combinations()` 中添加：

```python
{
    "name": "custom_config",
    "SAMPLES_PER_LEVEL": 3,
    "PREAMBLE_PATTERN_BYTES": 4,
    "DIFS_DURATION_MS": 20,
    "CW_MIN": 10,
    "CW_MAX": 200,
    "SLOT_TIME_MS": 5,
}
```

### 修改重复次数

在 `run_all_experiments()` 中修改 `range(4)` 为 `range(n)`：

```python
for repeat_idx in range(8):  # 改为 8 次重复
```

### 修改接收超时

在 `run_single_test()` 中修改 `-d` 参数的值：

```python
pm.start_process("rx2", ["cargo", "run", "--release", "--", "rx", "-l", "2", "-r", "1", "-d", "60"], "rx2.log")
```

## 许可证

与 trackmaker-rs 项目相同。
