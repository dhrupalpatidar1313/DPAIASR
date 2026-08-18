$ErrorActionPreference = "Stop"

Write-Host "Checking for CUDA toolkit (nvcc) and driver (nvidia-smi)..."
$nvcc = Get-Command nvcc -ErrorAction SilentlyContinue
$smi = Get-Command nvidia-smi -ErrorAction SilentlyContinue

if ($null -ne $nvcc -and $null -ne $smi) {
    Write-Host "CUDA toolkit detected. Building with CUDA support."
    cargo build --release --features cuda $args
} else {
    Write-Host "CUDA toolkit not fully detected. Building CPU-only fallback."
    cargo build --release $args
}
