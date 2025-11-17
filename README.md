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

Additionally, we found the provided device will get much more noise when output volume is over 30%, se we recommend playback `0.29` or `-17dB` and record `0.64` or `16dB` to get the bset result.

## Note for Linux Pipewire

Pipewire contins its default jack implementation, to dajust settings, use:

```bash
pw-metadata -n settings 0 clock.force-rate 48000
pw-metadata -n settings 0 clock.force-quantum 128
```