#!/bin/bash
set -e

echo "Checking for CUDA toolkit (nvcc) and driver (nvidia-smi)..."
if command -v nvcc &> /dev/null && command -v nvidia-smi &> /dev/null; then
    echo "CUDA toolkit detected. Building with CUDA support."
    cargo build --release --features cuda "$@"
else
    echo "CUDA toolkit not fully detected. Building CPU-only fallback."
    cargo build --release "$@"
fi
