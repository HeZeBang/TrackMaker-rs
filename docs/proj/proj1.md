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

## amodem 的实现

我们的 Python 原型参考了 amodem 的发送实现，这位我们后期尝试 OFDM、HRCSS 等留出了足够的空间。

当设置 `BITRATE=1` 时，将采用如下配置：

```py
    1: Configuration(Fs=8e3, Npoints=2, frequencies=[2e3]),
```

- 采样频率：8kHz
- 调制点数：2 （代表 BPSK，即 2 个相位）
- 载波频率：2kHz，1 个载波

```py
        self.sample_size = self.bits_per_sample // 8
        assert self.sample_size * 8 == self.bits_per_sample

        self.Ts = 1.0 / self.Fs
        self.Fsym = 1 / self.Tsym
        self.Nsym = int(self.Tsym / self.Ts)
        self.baud = int(1.0 / self.Tsym)
        assert self.baud * self.Tsym == 1

        ...
```

- `Ts`：采样周期
- `Fsym`：符号频率
- `Nsym`：每个符号的采样点数
- `baud`：波特率

在 config 初始化的时候，除了计算上面的基本信息，还会使用到 QAM constellation （正交幅度调制星座图） 作为绘图。

### 什么是 QAM？

QAM 通过结合幅度（AM）和相位（PM）调制，能通过一个载波信号传输更多的数据。

QAM 之所以被称为“正交”，是因为它使用两个正交（即相位相差 90 度）的载波信号，通常是同相分量 (I, In-phase) 和正交分量 (Q, Quadrature)。数据被分配到这两个分量上，然后它们的振幅被调整，最后叠加在一起形成最终的 QAM 信号。

### 什么是星座图？

QAM 星座图是一种二维图形表示，用于展示不同符号在 I（同相）和 Q（正交）平面上的位置。每个点代表一个特定的符号，这些符号通过调整载波的幅度和相位来传输数据。

星座图可以看作极坐标图，这其中，角度（phase）对应于信号的相位，而距离原点的距离（magnitude）对应于信号的幅度。

因此，纯的 BPSK 相当于一个 $0\degree$ 和一个 $180\degree$ 的点。而纯的 AM 则是两个不同长度的共方向的点。

而 QAM 由于结合了 AM 和 PM，因此星座图上会有多个点，且这些点既有不同的角度（相位），也有不同的距离（幅度）。常见的 16 QAM 通常是一个正方形阵列，共有 16 个点，以 4x4 的形式排列。

我们为了调试，沿用了星座图的调试方法，可以直观的看到信号的质量。

### 发送端

由于我们使用 1kbps 的配置，因此默认是 BPSK，所以星座图的点是 $-1j$ 和 $1j$。

载波的不同相位的计算公式为
$$
\exp\bigl(2j\pi f\,n\,T_s\bigr),\quad n = 0,1,\dots,N_{\mathrm{sym}}-1,\quad f \in F
$$

#### Pilot Prefix

首先会从 `equalizer.prefix` 中读取前导符（Pilot Prefix）。

这一段音频由 200 个前导符号和 50 个静默符号组成。值得注意的是，这里的前导是通过 1 来表示的，静默是通过 0 来表示的，也就是说，中间表示是一个含有 200 个 1 和 50 个 0 的数组，这些数组再通过 `write` 方法与预制的 `carrier` 载波相乘，形成最终的音频信号。

#### Silence Break

默认的静默长度是 50 个符号长度。这里相当于再次静默了 50 个符号。

#### Train Symbol

接下来是训练符号（Train Symbol），它是一个长度也为 200 的符号序列，主要作用是用于信道估计和均衡。这里生成一段用于信道均衡训练的 OFDM 符号。

这里用了星座图的四个轴的远端作为符号，分别是 $1, j, -1, -j$，并用一个 LFSR 伪随机数生成器来随机生成。

具体到代码，这里会调用 `equalizer.train_symbols` 方法来生成一个大小为 `Ncarrier * equalizer_length` 的符号数组，然后再通过 `equalizer.modulator` 方法将这些符号调制成音频信号。

#### Encoding

我们默认一个 Frame 包含最多 255 个 byte，这些 byte 由三个部分组成：

- Length：1 byte，表示 CRC32 + PHY Payload 的长度（0～254，最大就是 1111'1110）
- CRC32: 4 byte，表示 PHY Payload 的 CRC32 校验码
- PHY Payload：最多 255 byte，表示实际的数据（我们的 ECC 使用 Reed Soloman，将会被打包进入 Data 段，保证数据兼容性）

一共有两种 Frame：

- Data Frame：包含实际的数据
- EOF Frame：表示数据结束，一共 5 byte，包含了 Length（`0000'0100`）、CRC32（4 个 `0000'0000`）

#### Modulation

声波里只有复数载波的实部成分，虚部只是用来在基带里保持数学上的对称性，并不物理输出。

### 接收端

接收端使用 `AsyncReader` 实现非阻塞读取音频数据。

#### Detection

通过短时相干性检测（`dsp.coherence`）来检测找出 `equalizer.prefix` 对应的 200 × 1 sps 强载波，找到前导符的位置。

接下来将会返回一个迭代器，这个迭代器从训练信号之后的第一个样本点开始，实现了粗略同步。除此之外还会返回测得信号的幅用来自动增益，以及估计的频率偏移。

#### Correction

接下来会通过插值采样矫正频偏，并用 `gain` 反向放大，便于后续的均衡。

#### Equalization

在频偏和增益校正之后，接收端使用自适应均衡器来补偿信道失真：

- 提取训练信号段：在检测到导频后，保留 `equalizer_length` 个符号样本。
- 调用 `equalizer.train(signal, expected, order, lookahead)` 计算 FIR 滤波器系数。
- 用训练好的滤波器对后续样本进行滤波：`sampler.equalizer = lambda x: list(filt(x))`。

这样可以最大限度地恢复各子载波的幅度和相位。

#### Demodulation

均衡完成后，将时域音频流分帧到每个符号周期：

- 使用 `dsp.Demux` 按 `omegas` 和 `Nsym` 提取每个子载波的复数样本。
- 对每个载波样本调用 `MODEM.decode`，基于最小距离法（nearest‐neighbour）将接收到的符号映射回比特序列。
- 合并所有子载波的比特流，得到一个按帧顺序打平的完整比特迭代器。

#### Frame 解码

将连续比特流还原成字节并重组帧：

- 调用 `framing.decode_frames(bitstream)`：
  - 每 8 个比特打包成 1 字节。
  - 读取长度前缀和 CRC，校验通过后提取实际载荷。
  - 遇到 EOF 帧即结束。
- 将每个有效载荷写入输出，恢复原始数据流。

## 流式识别

这里我们特别研究一下如何流式识别

### 前导检测

Python 的原型实现非常方便，因为 sampler 本身是一个迭代器，amodem 按照符号长度连续的切块，并且计算每块与载波的相干度，达到阈值持续就认为检测到载波。

Rust 省略了流式识别，采用录制后处理，效果也不错。

```py
# detect.py
def _wait(self, samples):
    counter = 0
    bufs = collections.deque([], maxlen=self.maxlen)
    for offset, buf in common.iterate(samples, self.Nsym, index=True):
        if offset > self.max_offset:
            raise ValueError('Timeout waiting for carrier')
        bufs.append(buf)

        coeff = dsp.coherence(buf, self.omega)
        if abs(coeff) > self.COHERENCE_THRESHOLD:
            counter += 1
        else:
            counter = 0

        if counter == self.CARRIER_THRESHOLD:
            return offset, bufs
```

在检测到的载波附近，做一次相关搜索，精确定位前导开始的样本偏移。同时估计前导幅度与频偏，供后续采样器与增益/频率校正。

```py
def run(self, samples):
    offset, bufs = self._wait(samples)
    ...
    bufs = list(bufs)[-self.CARRIER_THRESHOLD-self.SEARCH_WINDOW:]
    n = self.SEARCH_WINDOW + self.CARRIER_DURATION - self.CARRIER_THRESHOLD
    trailing = list(itertools.islice(samples, n * self.Nsym))
    bufs.append(np.array(trailing))

    buf = np.concatenate(bufs)
    offset = self.find_start(buf)
    ...
    prefix_length = self.CARRIER_DURATION * self.Nsym
    amplitude, freq_err = self.estimate(buf[:prefix_length])
    return itertools.chain(buf, samples), amplitude, freq_err
```

```py
def find_start(self, buf):
    carrier = dsp.exp_iwt(self.omega, self.Nsym)
    carrier = np.tile(carrier, self.START_PATTERN_LENGTH)
    zeroes = carrier * 0.0
    signal = np.concatenate([zeroes, carrier])
    signal = (2 ** 0.5) * signal / dsp.norm(signal)

    corr = np.abs(np.correlate(buf, signal))
    ...
    index = np.argmax(coeffs)
    log.info('Carrier coherence: %.3f%%', coeffs[index] * 100)
    offset = index + len(zeroes)
    return offset
```

接下来再是静音等待和校正环节

### 结尾检测

帧化使用长度前缀 + CRC32，发送端最后发一帧空负载 EOF=b''。解码端在流式逐帧取数中遇到 EOF 即结束。

```py
def decode(self, data):
    data = iter(data)
    while True:
        length, = _take_fmt(data, self.prefix_fmt)
        frame = _take_len(data, length)
        block = self.checksum.decode(frame)
        if block == self.EOF:
            log.debug('EOF frame detected')
            return

        yield block
```

## 频率矫正

和 Sample 中用啁啾每个包调制一次的方式不同，我们使用训练信号这一个已知的信号，采用线性回归估算出频率偏差。

这个方法的好处在于，一旦我们的训练信号足够准确并建立连接，此后的所有包都可以省略对齐，并且减少空白、增加信息密度。这在我们的调试输出图中得到了验证，载波的偏离非常具有线性的平均规律。

## ECC

我们使用 `reed-solomon` 库作为实现，此后可能考虑使用带有 `AVX` 加速的同类实现。