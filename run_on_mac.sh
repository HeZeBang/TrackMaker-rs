#!/bin/bash

# Set the library path for JACK
export DYLD_LIBRARY_PATH="$HOME/homebrew/lib:$DYLD_LIBRARY_PATH"

cargo run "$@"
