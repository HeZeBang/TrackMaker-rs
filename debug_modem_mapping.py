#!/usr/bin/env python3

import sys
import os
import numpy as np

# 添加 amodem 路径
sys.path.insert(0, '/Users/zbhe/TrackMaker-rs/ref/amodem')

# 设置环境变量
os.environ['BITRATE'] = '1'

from amodem import config, dsp

def debug_modem_mapping():
    """调试Python MODEM的编码/解码映射"""
    
    cfg = config.bitrates[1]  # BITRATE=1
    print(f"🔧 Configuration:")
    print(f"   Symbols: {cfg.symbols}")
    print(f"   Frequencies: {cfg.frequencies}")
    
    # 创建MODEM
    modem = dsp.MODEM(cfg.symbols)
    
    print(f"\n📋 MODEM properties:")
    print(f"   Symbols: {modem.symbols}")
    print(f"   Bits per symbol: {modem.bits_per_symbol}")
    
    print(f"\n🔄 Encode map (bits -> symbol):")
    for bits, symbol in modem.encode_map.items():
        print(f"   {bits} -> {symbol}")
    
    print(f"\n🔄 Decode list (symbol -> bits):")
    for symbol, bits in modem.decode_list:
        print(f"   {symbol} -> {bits}")
    
    # 测试一些符号的解码
    test_symbols = [
        complex(0, -1),  # 0-1j
        complex(0, 1),   # 0+1j
        complex(-1, 0),  # -1+0j
        complex(1, 0),   # 1+0j
    ]
    
    print(f"\n🧪 Testing symbol decoding:")
    for sym in test_symbols:
        decoded_bits = list(modem.decode([sym]))
        print(f"   {sym} -> {decoded_bits}")
    
    # 测试我们在Rust中看到的符号
    rust_symbols = [
        complex(-0.000, 1.000),  # -0.000 + 1.000i
        complex(0.000, -1.000),  # 0.000 + -1.000i
    ]
    
    print(f"\n🦀 Testing Rust-observed symbols:")
    for sym in rust_symbols:
        decoded_bits = list(modem.decode([sym]))
        print(f"   {sym} -> {decoded_bits}")
        
        # 找到最接近的标准符号
        distances = [abs(sym - std_sym) for std_sym in modem.symbols]
        closest_idx = np.argmin(distances)
        closest_sym = modem.symbols[closest_idx]
        print(f"     Closest standard symbol: {closest_sym} (distance: {distances[closest_idx]:.6f})")

if __name__ == "__main__":
    debug_modem_mapping()
