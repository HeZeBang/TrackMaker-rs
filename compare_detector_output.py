#!/usr/bin/env python3

import sys
import os
import numpy as np

# 添加 amodem 路径
sys.path.insert(0, '/Users/zbhe/TrackMaker-rs/ref/amodem')

# 设置环境变量
os.environ['BITRATE'] = '1'

from amodem import config, common, detect

def analyze_python_detector():
    """详细分析Python检测器的输出"""
    
    # 读取测试文件
    with open('tmp/fresh_digits.pcm', 'rb') as f:
        data = f.read()
    
    samples = common.loads(data)
    cfg = config.bitrates[1]
    
    print(f"📁 Original file: {len(samples)} samples")
    
    # 跳过开头静音
    signal_iter = iter(samples)
    skipped_samples = list(common.take(signal_iter, int(cfg.skip_start * cfg.Fs)))
    print(f"⏭️  Skipped {len(skipped_samples)} samples (skip_start)")
    
    remaining_before_detector = list(signal_iter)
    print(f"📊 Samples going into detector: {len(remaining_before_detector)}")
    
    # 运行检测器
    detector = detect.Detector(config=cfg, pylab=common.Dummy())
    signal, amplitude, freq_error = detector.run(iter(remaining_before_detector))
    
    # 转换为列表
    signal_list = list(signal)
    print(f"🎯 Detector output length: {len(signal_list)} samples")
    print(f"🎯 Detector output symbols: {len(signal_list) // cfg.Nsym} symbols")
    
    # 检查信号的不同部分
    print(f"\n🔍 Signal analysis:")
    
    # 前面部分
    print(f"First 20 samples: {signal_list[:20]}")
    
    # 中间部分
    mid = len(signal_list) // 2
    print(f"Middle 20 samples (at {mid}): {signal_list[mid:mid+20]}")
    
    # 后面部分
    print(f"Last 20 samples: {signal_list[-20:]}")
    
    # 检查信号的变化
    unique_values = set()
    for i in range(0, len(signal_list), cfg.Nsym):
        chunk = signal_list[i:i+cfg.Nsym]
        if len(chunk) == cfg.Nsym:
            # 四舍五入到3位小数
            rounded_chunk = tuple(round(x, 3) for x in chunk)
            unique_values.add(rounded_chunk)
    
    print(f"\n📈 Unique {cfg.Nsym}-sample patterns: {len(unique_values)}")
    if len(unique_values) <= 5:
        for i, pattern in enumerate(sorted(unique_values)):
            print(f"  Pattern {i}: {pattern}")
    
    return signal_list, amplitude, freq_error

if __name__ == "__main__":
    print("🐍 Analyzing Python detector output\n")
    signal, amp, freq_err = analyze_python_detector()
    print(f"\n📊 Summary:")
    print(f"   Signal length: {len(signal)} samples")
    print(f"   Amplitude: {amp:.3f}")
    print(f"   Frequency error: {freq_err:.6f}")
