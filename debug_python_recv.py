#!/usr/bin/env python3

import sys
import os
import numpy as np

# 添加 amodem 路径
sys.path.insert(0, '/Users/zbhe/TrackMaker-rs/ref/amodem')

# 设置环境变量
os.environ['BITRATE'] = '1'

from amodem import config, main, common, dsp

def debug_recv():
    """调试版本的接收器，打印关键信息"""
    
    # 读取测试文件
    with open('tmp/fresh_digits.pcm', 'rb') as f:
        data = f.read()
    
    print(f"📁 Read {len(data)} bytes from PCM file")
    
    # 转换为样本
    samples = common.loads(data)
    print(f"🔢 Converted to {len(samples)} samples")
    
    # 获取配置
    cfg = config.bitrates[1]  # BITRATE=1
    print(f"⚙️  Config: {cfg.Fs}Hz, {cfg.Nsym} samples/symbol, {len(cfg.frequencies)} carriers")
    print(f"📡 Frequencies: {cfg.frequencies}")
    print(f"🎯 Symbols: {cfg.symbols}")
    
    # 模拟检测器处理
    from amodem import detect, common as amodem_common
    detector = detect.Detector(config=cfg, pylab=amodem_common.Dummy())
    
    # 跳过开头的静音
    signal_iter = iter(samples)
    common.take(signal_iter, int(cfg.skip_start * cfg.Fs))
    print(f"⏭️  Skipped {int(cfg.skip_start * cfg.Fs)} samples (skip_start)")
    
    # 运行检测器
    signal, amplitude, freq_error = detector.run(signal_iter)
    print(f"🎯 Detector result: amplitude={amplitude:.3f}, freq_error={freq_error:.6f}")
    
    # 转换为列表以便检查长度
    signal_list = list(signal)
    print(f"📊 Signal after detector: {len(signal_list)} samples")
    signal = iter(signal_list)  # 转回迭代器
    
    # 创建采样器
    from amodem import sampling
    freq = 1 / (1.0 + freq_error)
    gain = 1.0 / amplitude
    sampler = sampling.Sampler(signal, sampling.defaultInterpolator, freq=freq)
    
    print(f"🔧 Gain: {gain:.3f}, Freq correction: {freq:.6f}")
    
    # 创建接收器
    from amodem import recv as _recv
    receiver = _recv.Receiver(config=cfg, pylab=amodem_common.Dummy())
    
    # 创建符号流 - 这是关键部分！
    symbols = dsp.Demux(sampler, omegas=receiver.omegas, Nsym=receiver.Nsym)
    
    print(f"🧮 Demux filters shape: {receiver.omegas}")
    print(f"🧮 Nsym: {receiver.Nsym}")
    
    # 跳过训练序列，查看数据部分
    # 根据 Python 代码，训练序列包括：prefix + silence + training + silence
    from amodem import equalizer
    
    # 计算需要跳过的符号数
    prefix_symbols = len(equalizer.prefix)
    silence_symbols = equalizer.silence_length
    training_symbols = equalizer.equalizer_length
    total_skip_symbols = prefix_symbols + silence_symbols + training_symbols + silence_symbols
    
    print(f"📋 Training sequence breakdown:")
    print(f"   Prefix: {prefix_symbols} symbols")
    print(f"   Silence: {silence_symbols} symbols")
    print(f"   Training: {training_symbols} symbols") 
    print(f"   Silence: {silence_symbols} symbols")
    print(f"   Total skip: {total_skip_symbols} symbols")
    
    # 跳过训练序列
    print(f"\n⏭️  Skipping {total_skip_symbols} training symbols...")
    for i in range(total_skip_symbols):
        try:
            next(symbols)
        except StopIteration:
            print(f"❌ Ran out of symbols at {i}")
            return
    
    # 现在检查数据符号
    print(f"\n🔍 Data symbols (after training):")
    symbol_list = []
    for i, symbol_vector in enumerate(symbols):
        if i >= 50:  # 检查更多符号
            break
        symbol_list.append(symbol_vector)
        # symbol_vector 是一个数组，对于 BITRATE=1 只有一个元素
        sym = symbol_vector[0] if len(symbol_vector) > 0 else complex(0, 0)
        print(f"  Data Symbol {i}: {sym:.3f} (magnitude: {abs(sym):.3f})")
    
    # 检查符号的变化
    if len(symbol_list) > 1:
        unique_symbols = set()
        for sym_vec in symbol_list:
            sym = sym_vec[0] if len(sym_vec) > 0 else complex(0, 0)
            # 四舍五入到 3 位小数
            rounded = complex(round(sym.real, 3), round(sym.imag, 3))
            unique_symbols.add(rounded)
        
        print(f"🎨 Unique symbol patterns: {len(unique_symbols)}")
        for sym in sorted(unique_symbols, key=lambda x: (x.real, x.imag)):
            print(f"   {sym}")
    
    # 测试解调器
    print(f"\n🔄 Testing modem decode:")
    modem = dsp.MODEM(cfg.symbols)
    for i, sym_vec in enumerate(symbol_list[:10]):
        sym = sym_vec[0] if len(sym_vec) > 0 else complex(0, 0)
        # 解码单个符号
        decoded_bits = list(modem.decode([sym]))
        bit = decoded_bits[0] if decoded_bits else None
        print(f"  Symbol {sym:.3f} -> bits: {bit}")

if __name__ == "__main__":
    debug_recv()
