#!/bin/bash
source ./ref/amodem/.venv/bin/activate

echo "=== Testing Python amodem (self-test) ==="
echo "Hello, world!" | BITRATE=1 amodem send -o tmp/python_sample.pcm
BITRATE=1 amodem recv -i tmp/python_sample.pcm
echo

echo "=== Testing Rust amodem sender with Python receiver ==="
cat assets/think-different.txt | cargo run --bin amodem send -o tmp/rust_sample.pcm && \
    BITRATE=1 amodem recv -i tmp/rust_sample.pcm > tmp/decoded.txt && \
    echo "Original text:" && \
    cat assets/think-different.txt && \
    echo && \
    echo "Decoded text:" && \
    cat tmp/decoded.txt && \
    echo && \
    echo "Diff result:" && \
    diff assets/think-different.txt tmp/decoded.txt && \
    echo "✅ SUCCESS: Rust sender is compatible with Python receiver!" && \
    rm tmp/rust_sample.pcm tmp/decoded.txt