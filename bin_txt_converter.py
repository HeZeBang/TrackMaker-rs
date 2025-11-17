import argparse
import sys

def text_to_bits(text):
    """Convert text to a list of bits (0s and 1s)."""
    bits = []
    for byte in text.encode('utf-8'):
        for i in range(7, -1, -1):  # MSB first
            bits.append((byte >> i) & 1)
    return bits

def bits_to_text(bits):
    """Convert a list of bits to text."""
    if len(bits) % 8 != 0:
        raise ValueError("Bit list length must be a multiple of 8")
    bytes_list = []
    for i in range(0, len(bits), 8):
        byte = 0
        for j in range(8):
            byte = (byte << 1) | bits[i + j]
        bytes_list.append(byte)
    return bytes(bytes_list).decode('utf-8')

def encode(input_file, output_file):
    """Encode text file to binary file."""
    with open(input_file, 'r', encoding='utf-8') as f:
        text = f.read()
    bits = text_to_bits(text)
    # Pack bits into bytes
    byte_data = bytearray()
    for i in range(0, len(bits), 8):
        byte = 0
        for j in range(min(8, len(bits) - i)):
            byte = (byte << 1) | bits[i + j]
        byte_data.append(byte)
    with open(output_file, 'wb') as f:
        f.write(byte_data)

def decode(input_file, output_file):
    """Decode binary file to text file."""
    with open(input_file, 'rb') as f:
        byte_data = f.read()
    bits = []
    for byte in byte_data:
        for i in range(7, -1, -1):  # MSB first
            bits.append((byte >> i) & 1)
    text = bits_to_text(bits)
    with open(output_file, 'w', encoding='utf-8') as f:
        f.write(text)

def main():
    parser = argparse.ArgumentParser(description="Encode TXT to BIN or decode BIN to TXT")
    parser.add_argument('mode', choices=['encode', 'decode'], help="Mode: encode or decode")
    parser.add_argument('input', help="Input file")
    parser.add_argument('output', help="Output file")
    
    args = parser.parse_args()
    
    try:
        if args.mode == 'encode':
            encode(args.input, args.output)
            print(f"Encoded {args.input} to {args.output}")
        elif args.mode == 'decode':
            decode(args.input, args.output)
            print(f"Decoded {args.input} to {args.output}")
    except Exception as e:
        print(f"Error: {e}", file=sys.stderr)
        sys.exit(1)

if __name__ == "__main__":
    main()