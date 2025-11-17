# 命令行参数使用指南

trackmaker-rs 现在支持命令行参数来替代交互式 dialoguer 模式。

## 基本用法

### 传输文件（发送端）

```bash
./trackmaker-rs tx [-l LOCAL_ADDR] [-r REMOTE_ADDR] [--encoding SCHEME]
```

**参数说明：**
- `-l, --local <LOCAL_ADDR>` - 本地发送端地址（默认：1）
- `-r, --remote <REMOTE_ADDR>` - 远程接收端地址（默认：2）
- `--encoding <SCHEME>` - 线路编码方案（可选值：`4b5b` 或 `manchester`，默认：`4b5b`）

**示例：**
```bash
# 使用默认参数（本地地址1，远程地址2，4B5B编码）
./trackmaker-rs tx

# 指定本地地址为 10，远程地址为 20，使用 Manchester 编码
./trackmaker-rs tx -l 10 -r 20 --encoding manchester

# 简写形式
./trackmaker-rs tx -l 10 -r 20 --encoding manchester
```

### 接收文件（接收端）

```bash
./trackmaker-rs rx [-l LOCAL_ADDR] [-r REMOTE_ADDR] [--encoding SCHEME] [-d DURATION]
```

**参数说明：**
- `-l, --local <LOCAL_ADDR>` - 本地接收端地址（默认：2）
- `-r, --remote <REMOTE_ADDR>` - 远程发送端地址（默认：1）
- `--encoding <SCHEME>` - 线路编码方案（可选值：`4b5b` 或 `manchester`，默认：`4b5b`）
- `-d, --duration <DURATION>` - 接收持续时间，单位秒（默认：60）

**示例：**
```bash
# 使用默认参数（本地地址2，远程地址1，4B5B编码，60秒超时）
./trackmaker-rs rx

# 指定本地地址为 20，远程地址为 10，Manchester 编码，120秒超时
./trackmaker-rs rx -l 20 -r 10 --encoding manchester -d 120

# 快速接收，只需 30 秒
./trackmaker-rs rx -d 30
```

### 测试模式（不需要 JACK）

```bash
./trackmaker-rs test [--encoding SCHEME]
```

**参数说明：**
- `--encoding <SCHEME>` - 线路编码方案（可选值：`4b5b` 或 `manchester`，默认：`4b5b`）

**示例：**
```bash
# 使用 4B5B 编码进行环回测试
./trackmaker-rs test

# 使用 Manchester 编码进行环回测试
./trackmaker-rs test --encoding manchester
```

## 交互式模式

如果需要使用原本的交互式 dialoguer 模式，可以添加 `--interactive` 标志：

```bash
./trackmaker-rs --interactive
```

或者不提供任何子命令参数：

```bash
./trackmaker-rs
```

## 线路编码方案

支持以下线路编码方案：

| 方案 | 简写 | 说明 |
|------|------|------|
| `4b5b` | `4b5b-nrz` | 4B5B NRZ 编码（默认） |
| `manchester` | `manchester-biphase` | Manchester Bi-phase 编码 |

## 地址范围

- 本地地址和远程地址都是 `u8` 类型，范围为 `0-255`
- 通常建议使用 `1` 和 `2` 作为发送端和接收端的地址
- 在同一通信对中，发送端和接收端的地址应该不同

## 完整示例

### 场景 1：两台机器通信

**机器 A（发送端）：**
```bash
./trackmaker-rs tx -l 1 -r 2 --encoding 4b5b
```

**机器 B（接收端）：**
```bash
./trackmaker-rs rx -l 2 -r 1 --encoding 4b5b -d 120
```

### 场景 2：使用 Manchester 编码

**发送端：**
```bash
./trackmaker-rs tx --encoding manchester
```

**接收端：**
```bash
./trackmaker-rs rx --encoding manchester -d 60
```

### 场景 3：快速测试（无需 JACK）

```bash
./trackmaker-rs test --encoding 4b5b
```

## 获取帮助

```bash
./trackmaker-rs --help
./trackmaker-rs tx --help
./trackmaker-rs rx --help
./trackmaker-rs test --help
```
