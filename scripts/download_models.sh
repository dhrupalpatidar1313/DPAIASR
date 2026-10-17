#!/usr/bin/env bash
# Downloads the Silero VAD model required for voice-activity detection.
# (Whisper models are fetched and verified automatically by the app on first run.)
set -euo pipefail

DEST_DIR="$(cd "$(dirname "$0")/.." && pwd)/assets/models"
DEST="$DEST_DIR/silero_vad.onnx"
URL="https://github.com/snakers4/silero-vad/raw/v5.1.2/src/silero_vad/data/silero_vad.onnx"
SHA256="2623a2953f6ff3d2c1e61740c6cdb7168133479b267dfef114a4a3cc5bdd788f"

mkdir -p "$DEST_DIR"
if [ -f "$DEST" ] && echo "$SHA256  $DEST" | sha256sum -c --quiet 2>/dev/null; then
    echo "silero_vad.onnx already present and verified."
    exit 0
fi

echo "Downloading silero_vad.onnx ..."
curl -fL --retry 3 -o "$DEST.tmp" "$URL"
echo "$SHA256  $DEST.tmp" | sha256sum -c --quiet
mv "$DEST.tmp" "$DEST"
echo "Saved $DEST"
