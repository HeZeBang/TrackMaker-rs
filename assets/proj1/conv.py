# Read file and convert "1", "0"s to binary
def read_binary_file(filename):
    with open(filename, 'r') as f:
        content = f.read().strip()
    return bytes(int(content[i:i+8], 2) for i in range(0, len(content), 8))

# Write binary data to text file as "1", "0"s
def write_binary_file(filename, data):
    with open(filename, 'w') as f:
        for byte in data:
            f.write(f'{byte:08b}')

if __name__ == "__main__":
    # args
    import argparse
    # --tobin / --frombin <filename> > stdout
    parser = argparse.ArgumentParser(description='Convert binary file to/from "1", "0" text format')
    group = parser.add_mutually_exclusive_group(required=True)
    group.add_argument('--tobin', metavar='FILE', help='Convert text file to binary')
    group.add_argument('--frombin', metavar='FILE', help='Convert binary file to text')
    args = parser.parse_args()
    if args.tobin:
        data = read_binary_file(args.tobin)
        import sys
        sys.stdout.buffer.write(data)
    elif args.frombin:
        with open(args.frombin, 'rb') as f:
            data = f.read()
        write_binary_file(args.frombin + '.txt', data)