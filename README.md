# TrackMaker-rs
A high-performance audio-based information transmission tool, written in Rust

## Note on MacOS

To fully utilize JACK on macOS, you may need to install additional components such as `jack` via Homebrew:

```bash
brew install jack
```

Normally, the JACK server will start in 44100Hz with a buffer size of 512 samples. To change this settings, start the JACK server by:

```bash
jackd -d coreaudio -r 48000 -p 256
```

If you're launching this program on MacOS with homebrew, link the dynamic libraries by:

```bash
export DYLD_LIBRARY_PATH="$HOME/homebrew/lib:$DYLD_LIBRARY_PATH"
```

## Preparation

You should ensure `jack` is running.

- For Windows, install `jack` with `Qjackctl` and use it to start `jackd`.
- For MacOS, install `jack`, and use `jackd` command to start.
- For Linux, most distros contains `jack` support.

## Installation

1. Install `cargo`
2. Run `cargo build` to build and install dependencies
3. Use `cargo run` to run main programs, for example:
    - `cargo run send --input assets/proj1/test.bin --reed-solomon`
    - `cargo run receive --reed-solomon`
4. Use `cargo run --help` to show help files

## Examples

Use `cargo run --example [example]` to run files under `examples/`

- 

## Docs

See `docs/` folder for current implementation or visit [Website on `main` Branch](https://hezebang.github.io/TrackMaker-rs/)