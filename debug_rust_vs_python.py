#!/usr/bin/env python3

import sys
import os
import numpy as np

# 添加 amodem 路径
sys.path.insert(0, '/Users/zbhe/TrackMaker-rs/ref/amodem')

# 设置环境变量
os.environ['BITRATE'] = '1'

from amodem import config, main, common, dsp, detect, sampling, recv as _recv

def compare_demux_filters():
    """对比Python和Rust的Demux滤波器"""
    
    cfg = config.bitrates[1]  # BITRATE=1
    
    # Python的滤波器计算
    omegas = 2 * np.pi * np.array(cfg.frequencies) / cfg.Fs
    omega = omegas[0]
    nsym = cfg.Nsym
    
    print(f"🔧 Configuration:")
    print(f"   Omega: {omega}")
    print(f"   Nsym: {nsym}")
    print(f"   Frequencies: {cfg.frequencies}")
    
    # Python的exp_iwt函数
    def exp_iwt(omega, n):
        return np.exp(1j * omega * np.arange(n))
    
    # Python的滤波器：exp_iwt(-w, Nsym) / (0.5*Nsym)
    python_filter = exp_iwt(-omega, nsym) / (0.5 * nsym)
    
    print(f"\n🐍 Python Demux filter (first 8):")
    for i in range(8):
        print(f"   [{i}]: {python_filter[i]:.6f}")
    
    # Rust应该产生的滤波器（用于对比）
    print(f"\n🦀 Expected Rust filter (first 8):")
    for i in range(8):
        phase = -omega * i
        exp_val = complex(np.cos(phase), np.sin(phase))
        rust_filter_val = exp_val / (0.5 * nsym)
        print(f"   [{i}]: {rust_filter_val:.6f}")
    
    # 测试一个简单的信号
    test_signal = np.array([1.0, 0.0, -1.0, 0.0, 1.0, 0.0, -1.0, 0.0])
    
    correlation = np.dot(python_filter, test_signal)
    print(f"\n🧪 Test signal {test_signal} -> correlation: {correlation:.6f}")
    
    return python_filter

def debug_full_process():
    """调试完整的处理过程"""
    
    # 读取测试文件
    with open('tmp/fresh_digits.pcm', 'rb') as f:
        data = f.read()
    
    samples = common.loads(data)
    cfg = config.bitrates[1]
    
    # 模拟完整的接收流程
    signal_iter = iter(samples)
    common.take(signal_iter, int(cfg.skip_start * cfg.Fs))
    
    detector = detect.Detector(config=cfg, pylab=common.Dummy())
    signal, amplitude, freq_error = detector.run(signal_iter)
    
    signal_list = list(signal)
    print(f"📊 After detector: {len(signal_list)} samples")
    
    # 创建采样器和接收器
    freq = 1 / (1.0 + freq_error)
    gain = 1.0 / amplitude
    sampler = sampling.Sampler(signal_list, sampling.defaultInterpolator, freq=freq)
    
    receiver = _recv.Receiver(config=cfg, pylab=common.Dummy())
    symbols = dsp.Demux(sampler, omegas=receiver.omegas, Nsym=receiver.Nsym)
    
    # 检查前580个符号（对应Rust的输出）
    print(f"\n🔍 First 580 symbols from Python:")
    symbol_list = []
    for i, symbol_vector in enumerate(symbols):
        if i >= 580:
            break
        symbol_list.append(symbol_vector)
        sym = symbol_vector[0] if len(symbol_vector) > 0 else complex(0, 0)
        
        if i < 10 or i % 100 == 0:
            print(f"  Python Symbol {i}: {sym:.3f}")
    
    # 检查训练跳过后的符号
    if len(symbol_list) > 550:
        print(f"\n🎯 Data symbols (after skipping 550):")
        for i in range(10):
            idx = 550 + i
            if idx < len(symbol_list):
                sym = symbol_list[idx][0] if len(symbol_list[idx]) > 0 else complex(0, 0)
                print(f"  Python Data[{i}]: {sym:.3f}")
    
    print(f"\n📈 Symbol statistics:")
    all_symbols = [s[0] if len(s) > 0 else complex(0, 0) for s in symbol_list]
    unique_symbols = set()
    for sym in all_symbols:
        rounded = complex(round(sym.real, 2), round(sym.imag, 2))
        unique_symbols.add(rounded)
    
    print(f"   Total symbols: {len(all_symbols)}")
    print(f"   Unique patterns: {len(unique_symbols)}")
    print(f"   Patterns: {sorted(unique_symbols, key=lambda x: (x.real, x.imag))}")

if __name__ == "__main__":
    print("🔬 Comparing Python and Rust Demux implementations\n")
    compare_demux_filters()
    print("\n" + "="*60 + "\n")
    debug_full_process()
