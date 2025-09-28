#!/usr/bin/env python3

import sys
import os

# 添加 amodem 路径
sys.path.insert(0, '/Users/zbhe/TrackMaker-rs/ref/amodem')

from amodem.framing import BitPacker

def debug_bit_packing():
    """调试Python的比特打包逻辑"""
    
    packer = BitPacker()
    
    print("🔍 Python BitPacker analysis:")
    print("Byte size:", packer.byte_size)
    
    # 显示前16个字节的位模式
    print("\n📋 First 16 bytes and their bit patterns:")
    for i in range(16):
        bits = packer.to_bits[i]
        print(f"  Byte {i:02x} ({i:3d}): {bits}")
    
    # 测试一些具体的位模式
    test_patterns = [
        (0, 0, 0, 0, 0, 0, 0, 0),  # 0x00
        (1, 0, 0, 0, 0, 0, 0, 0),  # 0x01
        (0, 1, 0, 0, 0, 0, 0, 0),  # 0x02
        (1, 1, 0, 0, 0, 0, 0, 0),  # 0x03
        (1, 1, 0, 0, 0, 0, 1, 0),  # 0x53 = 83
        (1, 0, 1, 1, 1, 0, 1, 0),  # 0x5D = 93
    ]
    
    print("\n🧪 Testing specific bit patterns:")
    for pattern in test_patterns:
        if pattern in packer.to_byte:
            byte_val = packer.to_byte[pattern]
            print(f"  {pattern} -> 0x{byte_val:02x} ({byte_val:3d})")
        else:
            print(f"  {pattern} -> NOT FOUND")
    
    # 反向测试：给定字节值，看其位模式
    test_bytes = [0x00, 0x01, 0x02, 0x03, 0x53, 0x5D, 0x2B, 0xFF]
    print(f"\n🔄 Reverse lookup (byte -> bits):")
    for byte_val in test_bytes:
        bits = packer.to_bits[byte_val]
        print(f"  0x{byte_val:02x} ({byte_val:3d}) -> {bits}")
    
    # 分析位序
    print(f"\n📊 Bit ordering analysis:")
    print(f"  0x01 -> {packer.to_bits[1]} (LSB first)")
    print(f"  0x02 -> {packer.to_bits[2]}")
    print(f"  0x04 -> {packer.to_bits[4]}")
    print(f"  0x08 -> {packer.to_bits[8]}")
    print(f"  0x80 -> {packer.to_bits[128]}")

if __name__ == "__main__":
    debug_bit_packing()
